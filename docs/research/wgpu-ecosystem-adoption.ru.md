# Библиотеки, позволяющие строить качественный FLUI сейчас

Проверено 1 октября 2026 года. Горизонт 2031 — требование к расширяемости,
а не повод откладывать реализацию. Первоначальный поиск по downloads wgpu
выявил GPU-зависимости, но пропускал самые полезные независимые foundations.
Ниже решения по конкретным контрактам. Это исследование исходников/API и
registry manifests; production зависимости не изменены, сборки не выполнены.

## Первое внедрение: цвет и геометрия

**Принять color 0.3.3 для прототипа реального gradient lowering сейчас.**
У него есть `AlphaColor<Space>`, `PremulColor<Space>`, `DynamicColor`,
`convert`, `Interpolator` и `gradient`: это готовая математика color spaces,
alpha и подготовки gradient ramp, которой не должен заниматься вручную UI engine.
Прототип должен выдавать ramp для существующего Shader, доходить до GPU readback
и различать transparent saturated endpoint, linear sRGB и Oklab midpoint.
Принимаем после сравнения с независимыми CSS Color 4 fixtures, а не собственной
формулой теста. Space и alpha metadata сохраняются до engine boundary.
[API 0.3.3](https://docs.rs/color/0.3.3/color/) подтверждает DisplayP3,
Rec2020 и Oklab; ICC, tone mapping и platform HDR negotiation библиотека не решает.
Firecrawl дополнительно прочитал [исходный repository](https://github.com/linebender/color).

**Использовать уже принятый kurbo 0.13.x, не заменять его вторым path model.**
`flui-painting::Path` уже основан на BezPath; workspace ^0.13 допускает опубликованный
0.13.1. Следующий реальный шаг — единый flatten/tessellation tolerance в device
space и точный nested path clip через mask/stencil. Принятие: transformed curved
clip с hole, inverse fill, fractional DPR и save/restore должен различать
intersection от bounding rectangle. [Kurbo 0.13.1](https://docs.rs/kurbo/0.13.1/kurbo/)
не делает GPU clipping; существующий lyon остаётся triangulation helper.

## Восемь контрактов и их место в пяти деревьях

| Требование | Современный foundation и состояние | Проводка и ближайшая проверка | Что обязаны определить в FLUI |
|---|---|---|---|
| Цвет, прозрачность, градиенты | color **0.3.3**, Apache/MIT, без wgpu. peniko **0.6.1** использует color ^0.3.3/kurbo ^0.13.1; пока не workspace зависимости | View задаёт color intent → Painting Shader хранит space/alpha → Layer сохраняет их → engine готовит ramp. Начать color pilot выше; peniko Gradient/InterpolationAlphaSpace/Extend использовать как проверяемый vocabulary и отдельный adapter prototype | Интерполяционный space, premultiplication boundary, output format, HDR/SDR mapping, mixed-color images. peniko не переносить целиком без consumer |
| Path/clip | kurbo **0.13.1**, lyon **1.0.19** уже foundations без GPU | RenderObject Path → Layer clip → wgpu mask/stencil; analytic interior pixel fixtures для holes, transforms, intersections и save/restore | Device-space tolerance, fill rule, AA coverage и lifetime clip masks определяет FLUI; tessellator не compositor |
| Текст | Parley **0.11.1** уже flui-painting; glyphon **0.12.0** wgpu30, но использует cosmic-text и etagere | View text → Parley shaping/layout → RenderObject baseline → existing engine glyph atlas. Проверить bidi+fallback+variable font+scaled clipping через public harness/readback | Не вводить второй shaper ради удобного atlas. Glyphon полезен как atlas API/workload reference; adoption только если сохраняет Parley glyph contract и font identity |
| Flex/Grid layout | Taffy **0.14.0**, MIT, без GPU; пока не workspace | View typed layout config → RenderObject adapter `LayoutPartialTree`, `compute_flexbox_layout`/`compute_grid_layout`; не второе Element tree. Pilot grid с Parley measure, min/max constraints и fractional rounding | CSS semantics не равны нынешнему RenderFlex. Сохраняем existing Row/Column contract; новый grid container получает explicit sizing/baseline/intrinsics policy |
| Accessibility и automation wire | AccessKit **0.25.1** (workspace ^0.25), schemars **1.2.2** (workspace ^1.0) уже приняты | Semantics → AccessKit platform adapters; protocol serde/schemars → existing devtools/desktop-mcp. Acceptance stale target after replacement, disabled action, redacted text, accepted/committed/displayed wait | Schema не авторизует действие. Realm identity, action authority, capture privacy и frame acknowledgment остаются host contract; не новый parallel agent API |
| Shader ABI и диагностика | Naga **30.0.1**, naga_oil **0.23**, wgsl_bindgen **0.23**, bytemuck **1.25.2**, wgpu-profiler **0.28** уже foundations; encase **0.13.0** pilot | Engine build validates WGSL; generated bindings должны оставаться default. Encase storage array pilot ниже. Profiler wiring измеряет upload/pass/offscreen cost на capability-enabled adapter | ABI не гарантирует painter order. Нужны immutable per-draw data, submission/lifetime boundaries и measured budgets; profiler не production timing promise |
| Масштабирование resources и API recovery | etagere **0.3** уже atlas allocator; linebender_resource_handle **0.1.1** Apache/MIT даёт shared Blob/WeakBlob; proptest **1.11.0** уже ^1 workspace, loom **0.7.2** отдельный candidate | В engine проверить cache churn/device recreation; в existing protocol/desktop-mcp property sequences inspect→replace→invoke→close→recover. Blob adapter prototype только для actual shared resource consumer; Loom только малый scheduler/lease state model | Blob lifetime не GPU completion. Нужны owner+generation+descriptor(format,size,alpha,space)+submission retirement, bounded retained bytes/count, failure recovery. Allocator и ID crate не выберут admission/eviction policy |
| Provider streaming и AI interfaces | rig-core **0.43.0**, MIT, typed Model/Wire/Transport/Streamed; пока не dependency. Current ai_streaming использует manual reqwest SSE | Service IO adapter `Model::stream` → existing scheduler deliver → View StreamBuilder. Прототип повторяет существующую loopback fixture: chunks, replacement generation, Stop, 503, malformed frame, bounded body. Provider runtime вне build/layout/paint | Cancellation, secrets/endpoint policy, quotas, bounded queue и semantic authorization остаются FLUI/host. Не вводить inference в raster pipeline; отдельный transport candidate не оправдывает agent/memory dependency graph |

Taffy прямо рекомендует low-level API для framework с собственным деревом:
[API 0.14.0](https://docs.rs/taffy/0.14.0/taffy/). Это позволяет добавить mature
алгоритм, сохранив View/Element/RenderObject ownership. Не надо копировать CSS DOM.

[Peniko 0.6.1 API](https://docs.rs/peniko/0.6.1/peniko/) разделяет ImageData,
ImageSampler, ImageAlphaType, Mix/Compose и InterpolationAlphaSpace. Эти реальные
контракты полезнее заявления «поддерживаем HDR». Adapter должен сохранять информацию
без silent downgrade и проверять render result, а не наличие enum variant.

[Rig 0.43.0 API](https://docs.rs/rig-core/0.43.0/rig_core/) проверен по точному
crate module rig_core: `Model` связывает `Wire` и `Transport`; `stream` отдаёт
Streamed. Это реальная альтернатива ручному provider decoder, а не обещание
готового FLUI runtime. Минимальные features и wasm/mobile transport надо отдельно
проверить compilation/protocol fixture; здесь они не проверялись.

## Что реализовывать в первую очередь

1. color-backed gradient ramp + color/alpha metadata, с настоящим GPU readback.
2. kurbo→clip mask composition и independent analytic pixel fixtures, фиксирующие выявленные
   affine/path/ordering проблемы на единственном wgpu renderer.
3. Taffy grid RenderObject prototype с public layout/intrinsics/hit-test family.
4. Existing desktop-mcp protocol sequence properties и resource descriptor/lease
   admission/recovery contract, затем provider adapter comparison на текущем SSE fixture.
5. Encase оставлять лишь там, где measured ABI safety выигрывает у existing codegen.

Ни один из этих пунктов не требует ждать 2031. Срок горизонта определяет какие
metadata нельзя потерять сейчас; библиотека снимает математическую/алгоритмическую
работу, FLUI определяет интеграцию, authority и lifetime.


## Только компоненты для единственного wgpu renderer

Уточнение области: **Vello, включая CPU oracle, не предлагается к интеграции**.
Ранее проверенные manifests остаются историческим сравнением, не shortlist.
Основной renderer остаётся чистым wgpu. Полезны математические, geometry,
allocation, ABI, profiling и pixel-format компоненты вокруг его контрактов.

| Компонент | Точный API и compatibility | Решение и discriminating acceptance |
|---|---|---|
| glam **0.33.11**, MIT/Apache | Уже workspace ^0.33 и engine dependency. DMat/DAffine для f64, Mat/Affine для f32; latest optional encase ^0.12 и ^0.13 требуют feature selection | Использовать существующий glam в engine lowering. Public FLUI Point/Rect/Transform сохраняют f64 и единицы ADR-0098. Перевод в GPU f32 в одной boundary; test huge translation+small local quad, shear, reflection, projective w crossing zero |
| glamx **0.3.1**, MIT/Apache | Normal glam ^0.33.7, num-traits ^0.2, simba ^0.10; nalgebra ^0.35 optional. `MatExt::try_inverse()->Option`, `DSvd2/3`, `DPose2/3`, unit-complex `DRot2`; Pose только rigid rotation+translation | Прототип только для actual inverse consumer или singular-value-driven device-space tolerance. Не Pose для общего shear/nonuniform scale. Сравнить singular/near-singular/NaN matrices и inverse roundtrip с independent scalar fixture. try_inverse проверяет singularity, не гарантирует conditioning; epsilon, finite checks и projective clipping определяет FLUI |
| smallvec **1.16.2**, MIT/Apache | Уже workspace ^1.13, engine dependency; inline SmallVec и spill позволяют короткие resource/clip batches | Измерить allocations существующего workload, выбрать capacity по распределению. Не cap: over-capacity spill test обязателен. Большие inline capacities увеличивают stack/копирование; не менять Vec повсеместно |
| arrayvec **0.7.8**, MIT/Apache | Fixed-capacity ArrayVec и checked try_push; не заменяет динамический DisplayList | Принимать только для actual bounded ABI record/descriptor table с typed capacity error. Acceptance N/N+1 и recovery следующего frame; overflow не panic и не silent truncation. Нет нужды новой dependency без consumer |
| half **2.7.1**, MIT/Apache | f16/bf16 conversion; optional bytemuck supports packed staging values. Не color management и не HDR policy | Прототип Rgba16Float readback/producer output: preserve >1 values, negative finite channels, alpha endpoints. Независимые binary16 fixtures 0/1/min-subnormal/max-finite и row padding отличают conversion от reinterpretation. Для SDR8 path не добавлять |
| wgpu internal gpu-alloc / descriptor helpers | wgpu-core/hal уже владеют backend allocations; сторонний gpu-alloc не lease manager engine | Не второй GPU allocator. Engine-owned cache admission, descriptor validation и retirement implement поверх wgpu handles. Test retained frame, eviction, device recreation, stale producer generation; budgets измеряют allocations/uploads, а не только число IDs |

[glamx 0.3.1 MatExt](https://docs.rs/glamx/0.3.1/glamx/trait.MatExt.html)
и [общий API](https://docs.rs/glamx/0.3.1/glamx/) прочитаны через Keenable.
[glam 0.33.11](https://docs.rs/glam/0.33.11/glam/),
[half 2.7.1](https://docs.rs/half/2.7.1/half/),
[smallvec 1.16.2](https://docs.rs/smallvec/1.16.2/smallvec/),
[arrayvec 0.7.8](https://docs.rs/arrayvec/0.7.8/arrayvec/) — pinned primary API.
Версии/license/normal vs optional dependencies проверены crates.io API
`/crates/name` и `/crates/name/version/dependencies`; graph compile не выполнялся.

## Проверенные кандидаты

### Подготовленный encase pilot

В ignored `target/engine-audit/encase-pilot` создан отдельный workspace с
encase =0.13.0, bytemuck =1.25.2 и Naga =30.0.1. Он читает фактический engine
linear gradient WGSL, получает GradientStop member offsets/span через Naga,
сериализует два stops encase и проверяет semantic bytes по независимым WGSL
offsets. Проверяется второй array element: packed 20-byte records ошибочно
сдвинули бы его относительно обязательного 32-byte stride. Negative control
показывает разницу packed 40 bytes и правильных 64 bytes.

Команда для выполнения после окончания текущей сборки:

```text
cargo run --manifest-path target/engine-audit/encase-pilot/Cargo.toml
```

Пилот подготовлен, но пока **не запускался**. Он не является GPU readback и не
содержит benchmark. Проверяемый production GradientStop уже имеет правильное
explicit padding и Pod. Следовательно, encase не исправляет найденный дефект
этого layout и массовая миграция не обоснована. Его возможная ценность —
автоматически определять padding будущих сложных storage/uniform payloads,
после proof и измерения serialization/allocation cost. Статический vertex
layout по locations не следует подменять uniform layout правилами encase.

Все сведения о license ниже — metadata опубликованной версии, не юридическое
заключение. Дата release подтверждает активность публикации, не качество.

| Crate / версия / дата | Точная связь с wgpu 30 и license | Решение и цена |
|---|---|---|
| lyon 1.0.19, 2026-03-08 | Нет зависимости wgpu; MIT OR Apache-2.0. Уже lock 1.0.19, manifest ^1.0.16. | Использовать сейчас. Path tessellation готова; fill/clip geometry и coverage остаются ответственностью FLUI. Нет второго GPU stack. |
| etagere 0.3.0, 2026-03-18 | GPU-free; MIT OR Apache-2.0. Уже dependency/lock. | Использовать сейчас; tests eviction/grow/recorded slot lifetime, а не свой rectangle packer. Allocation ownership и budget FLUI сохраняет. |
| parley 0.11.1, 2026-08-16 | GPU-free; Apache-2.0 OR MIT. Уже workspace dependency, std enabled. | Использовать имеющийся shaping в painting; fallback/RTL/fonts проверять там. Не переносить shaping в engine ради wgpu search hit. |
| bytemuck 1.25.2, 2026-07-19 | GPU-independent; Zlib OR Apache-2.0 OR MIT. Уже lock, derive enabled. | Использовать для Pod vertex/instance uploads. Pod доказывает пригодность bytes, не WGSL uniform alignment; нужен generation/reflection/layout contract. |
| wgpu-profiler 0.28.0, 2026-07-31 | normal/dev wgpu ^30.0.0; MIT OR Apache-2.0. Уже optional dependency. | Применить существующий feature к measured scenes сейчас. Timestamp queries требуют adapter features; no-op без capability не означает измеренный GPU. |
| encase 0.13.0, 2026-09-19 | Нет normal wgpu; dev ^24.0.0 не попадает в consumer graph. MIT-0. | Ограниченный pilot сейчас: uniform/storage serialization по WGSL. Проверить matrix/glam support отдельно, reuse buffer и byte layout. Не мигрировать все bindings параллельно wgsl_bindgen без выгоды. |
| glyphon 0.12.0, 2026-07-09 | normal wgpu ^30.0.0, cosmic-text ^0.19, etagere ^0.3.0; MIT OR Apache-2.0 OR Zlib. | Технически совместим с device, но не принимать как текстовый engine: ADR-0067 уже убрал этот stack ради готового ShapedParagraph и общего draw order. Полезно читать generation usage tracker; возврат требует ADR и сравнительного proof. |
| rend3 0.3.0, 2022-02-12 | wgpu ^0.12, wgpu-core ^0.12.2, hal ^0.12.4; MIT OR Apache-2.0 OR Zlib. | Отклонить опубликованную версию для native same-device embedder. Огромный port/version gap, не drop-in 3D addition. Это оценка release, не утверждение об активности всех forks. |
| bevy_render 0.19.1 stable; 0.20.0-rc.2 от 2026-09-28 | stable normal wgpu ^29.0.3; RC normal wgpu ^30 плюс types ^30. MIT OR Apache-2.0. | RC совместим по major, но renderer интегрирован с Bevy app/ECS/assets/tasks. Для небольшого UI cube не добавлять весь stack. Для реального Bevy consumer — отдельный integration prototype shared device ownership/recovery и layer output, не renderer замена. |

## Почему downloads не определяют выбор

Запрошенная [страница поиска](https://crates.io/search?q=wgpu) и
[сортировка downloads](https://crates.io/search?q=wgpu&page=1&sort=downloads)
полезны для discovery. Извлечение второй страницы Keenable не удалось;
эквивалентный API запрос успешно прочитан. Первые результаты включают
android_system_properties, profiling, naga, wgpu-types/core/hal и platform
dependency bundles: это показывает transitive popularity, а не готовое
решение clip/text/3D. Не добавлять wgpu-hal/core напрямую ради downloads:
safe wgpu уже выбирает их согласованные версии и platform features.

## Проверка совместимости и платформ

Совпадение requirement ^30 не доказывает correct resource sharing. Для нового
пакета нужны cargo tree без второго wgpu major, feature audit backend/pass-
through, formats/usages, device-loss generation, frame ownership, platform
smoke и meaningful pixels. У GPU-free helpers отсутствие wgpu dependency —
преимущество совместимости, но не доказательство no_std/wasm без проверки.

Существующие engine features — additive wgpu backend passthrough, platform
defaults заданы target-scoped manifests. Новая библиотека не должна случайно
вернуть default backend features, шейпер в engine, platform types вверх или
дополнительный process-global resource owner. Для Bevy RC отдельно проверить
multi_threaded, webgpu/webgl и asset/task costs. Исторически проверенные Vello
manifests имеют version mismatch независимо от заявленной поддержки платформ.

wgsl_bindgen 0.23 и naga_oil 0.23 уже workspace/build pipeline: manifest
комментарий фиксирует naga ^30. Поэтому encase — сравнение узкого uniform
serialization подхода, не восстановление отсутствующей shader reflection.
wgpu 30 собственный PipelineCache остаётся первым кандидатом для warm startup,
с его capability/backend/driver ограничениями, без придуманного cache crate.

## Доказательства и воспроизводимость

Crates metadata и dependency payloads прочитаны по адресам вида:

```text
https://crates.io/api/v1/crates/encase
https://crates.io/api/v1/crates/glyphon/0.12.0/dependencies
https://crates.io/api/v1/crates/vello/0.10.0/dependencies
https://crates.io/api/v1/crates/bevy_render/0.19.1/dependencies
https://crates.io/api/v1/crates/bevy_render/0.20.0-rc.2/dependencies
https://crates.io/api/v1/crates?q=wgpu&page=1&per_page=15&sort=downloads
```

Для каждой строки таблицы запрашивались /crates/name и /name/version/dependencies,
выводились version/date/license/repository и normal/dev wgpu requirements.
Локальные evidence commands:

```text
rg -n 'lyon|etagere|wgpu-profiler|encase|bytemuck|parley|glyphon|vello' Cargo.toml
rg -n 'wgsl_bindgen|naga_oil|wgpu =' Cargo.toml
rg -n 'name = "(lyon|etagere|parley|wgpu-profiler|bytemuck)"' Cargo.lock
```

Дополнительные первичные источники:

- [encase README: WGSL layout, buffers и ShaderType](https://github.com/teoxoy/encase), Keenable full content до examples truncation.
- [glyphon source/release activity](https://github.com/grovesNL/glyphon), Firecrawl; generation-based usage tracker от 9 июля 2026.
- [Vello repository](https://github.com/linebender/vello), первичный README прочитан в [предыдущем исследовании](engine-rust-competitors.ru.md); изменяемый main не заменяет registry dependency metadata.
- [wgpu 30 API](https://docs.rs/wgpu/30.0.1/wgpu/), Firecrawl developer search.
- [Принятое отделение glyph atlas](../adr/ADR-0067-engine-owned-glyph-atlas.md).

Последний ADR link и версии взяты из checkout; build compatibility здесь
не проверялась. Дополнительный проверяемый шаг — encase pilot и profiler
измерения имеющимися средствами, а не массовое добавление зависимостей.
