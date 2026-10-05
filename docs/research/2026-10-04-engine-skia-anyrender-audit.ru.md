# Графический конвейер FLUI: аудит Skia и AnyRender

## Решение и границы

Первый рекомендуемый шаг — корректность **foreground footprint**: сохранять
входные пиксели вне viewport, влияющие на видимый Blur, и не обрезать промежуточный
halo цепочки. Сохранить прямой wgpu-путь и существующие record/replay, клипы,
эффекты и ресурсные квоты. Новые backend traits, Skia/AnyRender dependencies,
backdrop-фильтры, новый affine blur и массовая перестройка API не нужны.

Это аудит и предложение следующей задачи. Production-исправление и PR не
подготовлены: пользователь сначала выбирает следующий шаг. Исследовательский
probe временно подключается к существующему painter test target; диагностические
счётчики не меняют растеризацию. Он печатает все случаи и завершается **FAIL**
при нарушении crop/support-контракта. Исходники восстанавливаются точными байтами.

## Исходное состояние и пересечения

| Объект | Зафиксированное состояние |
|---|---|
| FLUI | `dca90bac047e950b85943b617cccf460f723f894`, чистый detached worktree при старте; создана `codex/engine-filter-audit` |
| Remote main при проверке | Тот же SHA, `gh api repos/vanyastaff/flui/commits/main --jq .sha` |
| Skia | `8643b1d64cff21b5e6f8d65ca98204c6eecb0098` |
| AnyRender | `870407d142a2cd38ea6404717c6d68dceeeb91d6` |
| [PR #1420](https://github.com/vanyastaff/flui/pull/1420) | OPEN, `c8f22043ec4c7ae5b0716398e01701cdbc184cee`: inert panic payload retirement, renderer cancellation/Miri, gates. Исключён из реализации |
| [PR #1421](https://github.com/vanyastaff/flui/pull/1421) | MERGED в исходный SHA: completed decode admission, LRU eviction/recency и lru 0.18.5. Не повторять |
| [PR #1418](https://github.com/vanyastaff/flui/pull/1418), [#1419](https://github.com/vanyastaff/flui/pull/1419) | Уже в baseline: glyph recovery и общий UI research; reqwest pools/asset identity и dependency API audit |

Финальная повторная проверка: #1420 по-прежнему OPEN, но head уже
`598590c7e4eaaf0543ccb74cf323cf17469c2670`; #1421 по-прежнему MERGED.
Таблица выше сохраняет начальный snapshot. Перед выбранной реализацией снова
сверить текущие PR/diffs; ни одна ревизия чужой работы сюда не переносилась.

Команды: `git status --short`, `git rev-parse HEAD`, `git worktree list`,
`gh pr view 1420/1421 --json ...`, `gh pr list --state open`, `cargo xtask --help`.
Другие worktree сессий обнаружены; их исходники не менялись. Прочитаны
корневой AGENTS.md, engine/painting architecture, текущие ресурсные и UI-аудиты.
По `rg --files -uu -g SKILL.md` repository-specific skills не обнаружены.
Доступные artifact/web skills не нужны для локального source-code аудита:
upstream прочитан напрямую из Git. AGENTS.md предписывает разделять большой
аудит; три read-only агента исследовали непересекающиеся темы, их доказательства
сопоставлены с кодом.

Upstream shallow/sparse clones находятся вне FLUI в
`C:/Users/vanya/.codex/research/engine-filter-audit/{skia,anyrender}`.
Skia не собиралась. Здесь и далее ссылки upstream закреплены на SHA,
а ссылки на FLUI относятся к baseline, не к движущемуся main.

Предыдущий [engine capability audit](engine-capability-audit.ru.md) исторический:
его утверждения об отсутствии точных clips, headless masks/backdrop и неполной
affine image placement не переносятся в настоящее. Ни одно старое TODO не
считается доказательством отсутствия возможности.

## Реальный путь кадра

```text
UiRealm build/layout/paint → Canvas → DisplayList → LayerTree/Scene
  native: SceneSnapshot → RasterLane → RasterOwner::pump → Renderer
  web: DirectSink → Renderer
Renderer / FrameProtocol: damage plan → surface / retained target
  → shared layer_walk → LayerRender / LayerDispatcher
  → WgpuPainter / DrawBatcher → DrawSegment → SealedSegment / ordered DrawItem
  → GpuReplay: resource preparation, clip masks, nested offscreens, effects
  → DeviceDomain: prepare → submit → completion-owned charges/leases
  → optional retained-target blit → pre-present hook → present
```

| Стадия и FLUI-источник | Владелец и lifetime | Координаты | Инвалидирование / отказ |
|---|---|---|---|
| [Canvas/DisplayList](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-painting/src/display_list/mod.rs#L34) | Owned команды, Paint/path/image значения; paragraph удерживает Arc shaped data/font bytes. Не ссылки на frame-local font table | Logical f64 geometry, recorded CTM | Изменение paint/layout обновляет scene; volatile extents отдельно от cached bounds |
| [SceneSnapshot](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-layer/src/scene_snapshot.rs#L157), [RasterLane](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-app/src/app/raster_lane.rs#L322) | Owned Scene, damage, realm/presentation/epoch/generation stamp; snapshot до retirement pump | Scene local + root DPR; damage device | Supersession, stale owner/generation и resize не означают успешный present |
| [Renderer::render_frame_inner](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/renderer.rs#L1732), [FrameProtocol::run](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/frame_protocol.rs#L288) | Renderer владеет domain/surface/retained target. Candidate становится committed после успешной работы | Device framebuffer, damage scissor | Skip/direct/retained; resize/format/recovery/foreign scene сбрасывают reuse; failed frame оставляет retry debt и прежний committed image |
| [layer_walk](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/layer_walk.rs#L89), LayerRender/Dispatcher | Итеративный walk; effects могут consume subtree; painter state имеет balanced saves | CTM local→device, clips сохраняют membership + mapping | Invalid affine/projective clip admission даёт error; это уже не bounding-box-only clipping |
| [WgpuPainter](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/painter/mod.rs#L399), batches/command_ir | Mutable recording становится SealedSegment; nested items держат sealed input. GPU images/glyphs уже готовятся при recording | Geometry rebased в f64 перед f32 GPU packing; typed attachment remap | RecordingBudget sticky first error до нового frame; recording seam не полностью GPU-free |
| [GpuReplay / layer_offscreen](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/layer_offscreen.rs#L58) | Replay scratch отдельно charged; viewport/gradient/clip uniforms immutable per flush; RAII pool allocations | Attachment pixel lattice, root↔offscreen map, clip-local inverse | Clips, opacity, advanced blend, blur/morph/color, backdrop поддержаны. Nested foreground пока full-viewport fallback |
| [DeviceDomain::submit_and_track](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/device_domain.rs#L351) | Permits, external leases и bookkeeping до queue completion; uncertain failure quarantines ownership | GPU command buffers | Admission до выделения покрытых ресурсов; CPU finish не GPU completion |
| [present](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/renderer.rs#L1801), painter maintenance | Surface texture до present; buffers/uniform cursors и atlas maintenance после последнего submit/discard | Surface device pixels | Hook только для present; unmanaged render invalidates retained reuse |

**RasterOwner уже production-wired.** Native desktop
[runner](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-app/src/app/runner/desktop.rs#L300)
устанавливает RasterLane; Android/iOS делают то же. Lane submit-and-pump вызывает
RasterOwner, который вызывает backend.render_scene. Это inline synchronous
baseline, а не обещанный будущий raster thread. Web DirectSink — отдельный
намеренный путь. Устаревшее описание «пока не подключён» неверно.

Headless использует общий ordered layer recording/replay, но свой surface-less
device и RGBA8Unorm target. Он не проверяет acquire/present, window format,
native run-loop, frame pacing или device recovery на живом окне.

## Конкретные сравнительные контракты

| Контракт | Upstream на закреплённой ревизии | FLUI сейчас и вывод |
|---|---|---|
| Source / required input / output / final clip | [SkBlurImageFilter::onFilterImage, input/output bounds](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/src/effects/imagefilters/SkBlurImageFilter.cpp#L155) запрашивает child input для расширенного desired output; output определяется независимо | FLUI Blur/Morph/Chain смешивают source AABB и viewport-clip; подтверждённый дефект ниже |
| Filter coordinate spaces | [SkImageFilterTypes mapping](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/src/core/SkImageFilterTypes.h#L98), [matrix capability](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/src/core/SkImageFilterTypes.h#L548), [mapSigma](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/src/effects/imagefilters/SkBlurImageFilter.cpp#L198) | Foreground sigma передаётся raw в device-pixel convolution; backdrop maps CTM. Различие установлено, публичная система единиц недостаточно определена. Смена semantics — отдельная задача |
| saveLayer restore order | [SkCanvas::internalDrawDeviceWithFilter](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/src/core/SkCanvas.cpp#L699): image filter → alpha/color filter → blender | FLUI имеет isolated group, ordered effects, color filters и composite blend. Parent group opacity применяется при parent composite. Отброшенная локальная переменная opacity в Filter-arm сама по себе не доказывает потерю group opacity |
| Vertices UV/color/blending | [SkCanvas::drawVertices](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/include/core/SkCanvas.h#L2062), [SkDraw_vertices](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/src/core/SkDraw_vertices.cpp#L90) проверяет actual vertex opacity; shader/color combination отделена от destination blend | FLUI UV записывает, но shape shader не использует; vertex alpha неправильно выбирает opaque route. Две конкретные задачи, не повод переносить Skia API целиком |
| GPU cache budget / in-flight | [Graphite ResourceCache](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/src/gpu/graphite/ResourceCache.cpp#L156), [purge](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/src/gpu/graphite/ResourceCache.cpp#L615) различает nonpurgeable и purgeable; удаляет только допустимые LRU | FLUI prepared quotas и completion ownership существуют. Они не охватывают resident atlas/cache/pool память; нельзя называть их whole-engine VRAM budget |
| Atlas hazard discipline | [Graphite DrawAtlas](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/src/gpu/graphite/DrawAtlas.h#L53): use tokens, uploads, plot generations | FLUI защищает this-frame slots, сохраняет grow coordinates, выполняет ordered writes/draws на одной Queue. Подтверждённого use-after-reuse не найдено; не нужен fence на каждый glyph |
| Snapping / AA | [Graphite snap_rect_to_pixels](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/src/gpu/graphite/Device.cpp#L175), [drawRect](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/src/gpu/graphite/Device.cpp#L913): restricted transform, round-trip checks, non-AA policy | FLUI text уже выбирает device-grid placement для uniform positive CTM. Fractional content сохраняет AA; blanket rounding изменит animation/rotation. Выбрать policy по ADR-0098, затем pixel witnesses |
| Record / prepare / submit | [Recorder::snap](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/src/gpu/graphite/Recorder.cpp#L193), [Context::insertRecording/submit](https://github.com/google/skia/blob/8643b1d64cff21b5e6f8d65ca98204c6eecb0098/src/gpu/graphite/Context.cpp#L265) | FLUI уже Scene IR → sealed command IR → replay → DeviceDomain completion. Полезно измерять cost/failure каждой стадии; второй backend interface не решает найденные дефекты |
| Owned scenes / glyph runs | [AnyRender RenderCommand/Scene/GlyphRunCommand](https://github.com/DioxusLabs/anyrender/blob/870407d142a2cd38ea6404717c6d68dceeeb91d6/crates/anyrender/src/recording.rs#L13), [append_scene](https://github.com/DioxusLabs/anyrender/blob/870407d142a2cd38ea6404717c6d68dceeeb91d6/crates/anyrender/src/lib.rs#L261) | AnyRender owns BezPath/brush/filter/font/glyph vectors; FLUI owns analogous commands/fonts/shaped runs. Наличие AnyRender multi-backend trait не создаёт потребности FLUI в таком trait |
| Unsupported brushes / resource lifetime | [AnyRender convert_paint](https://github.com/DioxusLabs/anyrender/blob/870407d142a2cd38ea6404717c6d68dceeeb91d6/crates/anyrender/src/recording.rs#L165), [Resource/Custom fallback](https://github.com/DioxusLabs/anyrender/blob/870407d142a2cd38ea6404717c6d68dceeeb91d6/crates/anyrender/src/types.rs#L78), [Skia age cache](https://github.com/DioxusLabs/anyrender/blob/870407d142a2cd38ea6404717c6d68dceeeb91d6/crates/anyrender_skia/src/cache.rs#L43) | Transparent fallback — предостережение, а не образец FLUI. Age eviction не byte budget; ResourceId не удерживает внешний ресурс. FLUI external draw уже captures allocation lease, update/unregister не перенаправляет recorded draw |

## Приоритеты: доказательство, исправление, тест

P0 — первая очередь correctness, не заявление о crash/security severity.
«По коду» означает конкретный воспроизводимый сценарий с полным source trace,
но не исполненный GPU oracle. Гипотезы перечислены отдельно.

### P0: входной footprint преждевременно ограничен viewport

**FLUI:** `WgpuPainter::restore_layer`,
[painter/layer.rs:952](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/painter/layer.rs#L952)
пересекает content AABB с viewport до growth; :959 пересекает grown снова.
`filter_fb_rect` :167–185 использует unsigned origin и clamped far corner.
`render_filter_input` / grown-offscreen затем не могут растеризовать внешний input.
Это касается Blur, Morph и Chain; unsupported-AABB fallback тоже viewport.

**Сценарий:** viewport 32×32, чёрный rect `[-6,-2) × [10,14)`, sigma 4.
Радиус 7: source за левым краем влияет на столбцы 0…4. В viewport 96×96 с
переносом всей сцены на (32,32) crop центра должен сохранить этот halo.
Малый путь теряет source. Последствие — исчезающий halo, неправильные края
окна/partial target и corner blur. Skia input/output bounds выше демонстрируют
разделение областей; его коэффициент radius не является решением FLUI.

**Минимальное исправление:** signed root-space attachment origin, отдельные
source support, required input, filter output и final clip; finite working area
получить обратным распространением нужного output через actual FLUI kernels.
Применять final clip только при composite. Согласованно изменить remap всех
поддержанных draw kinds и nested operations; unsigned width/height сохранить.
Нельзя только убрать `.intersect(viewport)` или выделить texture всего source AABB.

**Тест:** small-vs-large crop по всем краям/углам, отрицательный outside-radius,
fractional placement, anisotropic/axis-zero sigma, supported scale, nested
clips/layers, прямой painter и SceneBuilder/Canvas replay. Матрица ниже.

### P0: цепочка повторно обрезает halo исходным bounds

**FLUI:** [apply_image_filter_passes](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/layer_offscreen.rs#L1175)
передаёт неизменный `content_bounds` каждому Blur/Morph. Blur H-pass
[blur/mod.rs:120](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/blur/mod.rs#L120)
создаёт source decal по этому bounds; shader исключает halo первого прохода.

**Сценарий:** Compose[Blur(2),Blur(2)] узкого rect внутри viewport. Второй Blur
должен сворачивать весь результат первого, включая halo. Выделенная область
уже суммирует growth, но actual pixels не соответствуют этой области.

**Минимальное исправление:** вести input/output support каждого прохода; не
декалить второй Blur к первоначальному source. Для color matrix, меняющей
transparent black, определить layer-domain, а не считать любой matrix локальным
bounds-preserving фильтром. Не внедрять новые filters.

**Тест:** отдельный независимый sequential convolution oracle или эквивалентность
двум вложенным Blur layers внутри большого viewport. Один crop oracle недостаточен:
оба render размера могут одинаково recrop halo. Сравнить edge pixels, не центры.

### P1: непрозрачный pipeline при прозрачных vertex colors

**FLUI:** [DrawBatcher::draw_vertices](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/batches/paths.rs#L487)
использует actual vertex colors, но
[pipeline_key_from_paint](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/pipeline_cache.rs#L750)
выбирает blend только по `paint.color.a`; opaque route имеет `blend=None`.
На синем opaque фоне red mesh alpha 128 с opaque Paint перезаписывает
destination premultiplied red/alpha128 вместо SrcOver purple/alpha255.
Skia opacity predicate читает actual vertex data, см. таблицу.

**Минимальное исправление:** conservative SrcOver alpha pipeline для mesh с
translucent actual colors, сохранив explicit blend. **Тест:** uniform/mixed vertex
alpha над nonblack backdrop, direct и Canvas replay, assert RGB и alpha;
counterfactual restores opaque choice. Пока подтверждено по коду, GPU не выполнен.

### P1: UV/shader mesh запрос молча превращается в solid draw

**FLUI:** `Canvas::draw_vertices` advertises tex coords, `Vertex` их содержит,
но [shape.wgsl:35](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/shaders/shape.wgsl#L35)
оставляет UV unused; `paths.rs:524` всегда solid pipeline. Gradient Paint на mesh
также игнорируется. Кроме того,
[ImageShader](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-painting/src/paint/shader.rs#L285)
не содержит image/resource identity: существующий API даже не называет source.

**Минимальное исправление:** явный unsupported recording error при запросе
неподдержанной mesh shader/UV semantics, error до публикации команды, здоровый
следующий frame. Это API-visible изменение: отдельно от filter fix.
**P2 full textured mesh:** только после use case определить UV units, sampler,
tile/filter quality, source ownership/alpha, vertex color composition и final blend.
Skia shader-vs-color blender не копировать без необходимости. Pixel test:
асимметричная 2×2 texture, переставленные UV, tint/alpha и backdrop; exact
unsupported error сейчас. По коду подтверждено, GPU не выполнен.

### P1: Solid shader игнорирует свой цвет

**FLUI:** [dispatch_shader_rect](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/batches/gradients.rs#L958)
возвращает false для Solid/Image, shapes fallback читает Paint color.
`Paint::fill(RED).with_shader(Shader::solid(BLUE))` rect рисует RED.
Это внутреннее противоречие explicit Solid shader, а не требование повторить Skia.
Upstream vertex shader/paint contract в таблице показывает явность source выбора.
**Минимальное исправление/тест:** resolve effective solid shader color перед
batching, assert BLUE с partial alpha над colored backdrop в producer/replay.
Согласовать с API-аудитом, не включать в первый шаг. По коду, без GPU witness.

### P1: полнота учёта resident GPU ресурсов

**FLUI:** [GlyphAtlas::create_page_texture](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/glyph_atlas.rs#L267)
raw creates/grows pages без Domain permits; страницы не shrink.
Prepared defaults 256 MiB GPU/128 MiB CPU — полезная квота covered payload,
не ограничение всего engine. При max-dim 16384 два glyph pages теоретически
занимают 256 MiB R8 + 1024 MiB RGBA8 вне этой квоты. Это арифметический upper
bound, не измеренное выделение и не доказанный OOM на устройстве.

TextureCache имеет реальный 100 MiB soft budget, но end-frame eviction защищает
active entries; `memory_bytes` считает occupied image texels, не полный atlas
storage. Pool idle allocations, atlas gutters, retained targets и external
storage тоже требуют distinct inventory. Graphite учитывает budgeted resources
и nonpurgeable references, а не обещает физический driver VRAM cap.

**Минимальный следующий scope:** allocation-identity inventory, resident glyph
page admission до growth, completion-owned old-page retention; далее image/pool
coverage отдельным шагом. **Тест:** tiny injected budget refuses growth до allocation,
старые recorded glyphs остаются видимыми, recovery сохраняет progress, charge
retirement после completion. Не ломать glyph recovery из #1418. Source gap
подтверждён; физический peak GPU-memory здесь не измерен.

### P1: capability contract supplied wgpu device

**FLUI:** [WgpuPainter::new/with_shared_device](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/painter/mod.rs#L148)
возвращают Self и eagerly создают PipelineSet без preflight minimum limits/formats.
Valid wgpu Device, запрошенный с `max_vertex_attributes=8`, не может создать
eager instanced rect layout: quad + 10 instance attributes; radial имеет 13
attributes всего и max location 13. Результат — wgpu validation path вместо
typed FLUI unsupported-device result. Это вывод по конкретным layouts, не
исполненный reduced-device эксперимент.

Renderer корректно negotiates optional dual-source/timestamp features; portable
blend fallback существует. Adapter support и enabled device features различаются.
Headless default-limit device и windowed negotiated device — разные доказательства.
**Минимальное исправление/тест:** explicit device/target minima preflight и typed
construction error до pipeline allocation; deliberately reduced device + healthy
next construction. Skia Graphite разделяет resource preparation failure от submit;
применима явность admission, а не backend abstraction. Смена public constructor
signature требует отдельного API scope и согласования с параллельным аудитом.

### P2: snapping, atlas stress и performance observations

Content snapping по ADR-0098 ещё требует static/animated policy; text device-grid
placement уже есть. Решение Skia non-AA snapping зависит от transform и precision;
не округлять всю геометрию. **Следующая задача:** policy + fractional DPR,
reflection/rotation, stroke, text и animated-edge witnesses; при смене cross-crate
контракта ADR. Не мешать pixel-cover offscreen bounds с snap contents.

Подтверждённой atlas in-flight corruption не найдено: this-frame slot protection,
одна Queue и ordered writes достаточны для текущего протокола. Дополнительный stress
test multiple unsent flushes / grow / next-frame reuse полезен, но это coverage,
не установленный bug. Перенос token topology Skia необязателен.

Graphite stage separation уже имеет соответствие FLUI. Перед оптимизациями
измерить CPU record/prepare/submit, GPU timestamps, cache-hit/miss, actual passes,
dimensions, resident allocation high-water и completion backlog. Anisotropic
per-axis bounds и sampler/layout reuse — кандидаты performance, пока без speedup claim.

## Actual foreground support и ограниченный дизайн

FLUI [kernel_radius](https://github.com/vanyastaff/flui/blob/dca90bac047e950b85943b617cccf460f723f894/crates/flui-engine/src/effects/blur.rs#L38)
и WGSL используют `rx=ceil(sigma_x*1.7320508)`, `ry=ceil(sigma_y*1.7320508)`.
H/V — integer tap convolution с normalized weights, premultiplied encoded RGBA;
нет foreground downsample. Linear sampler при integer-origin 1:1 attachment
grid должен попадать в texel centers. Crop oracle проверит numerical/subtexel
ошибки. Bounds cover AA rasterized pixels на integer lattice; content не snap.
Нельзя использовать Skia `3*sigma` вместо фактической поддержки FLUI.

Обозначения: `S` — support source после внутренних clips; `C` — final visible
clip (viewport, inherited group clip, damage); `O` — forward output support;
`R` — required input для наблюдаемого output. Для blur `O=S⊕(rx,ry)`;
`V=O∩C`; `R=V⊕(rx,ry)` до пересечения с доступным source. Для цепочки backward
propagation идёт outer→inner, forward support — inner→outer. Offscreen working
domain включает нужные intermediate outputs, а не только `R∩S`.

Первый conservative вариант может использовать общий integer-covered working
rect вокруг final visible domain с суммой per-axis support radii всей цепочки.
Дальняя geometry вне обратного reach не увеличивает texture; размеры ограничены
`visible_width + 2*sum(rx)` и аналогично Y плюс явно обоснованный raster cover
padding, device max dimension и existing admission. При превышении — явный
resource error, без clamp, меняющего пиксели. Unknown AABB kinds fallback на
**bounded required working domain**, не source union и не прежний viewport.

Нужен единый signed root→attachment remap для flat и nested replay, UV/decal,
scissors и clip masks. Сейчас `render_filter_input` nested asserts origin=(0,0),
dim=viewport; `GpuStateStack` преждевременно clamps scissors к original surface.
Поэтому исправление signed `filter_fb_rect` в одиночку не готово. Все existing
primitive paths, images/gradients/glyphs и layer children должны сохранить pixels;
это bounded внутрикратное изменение coordinate plumbing, без новой архитектуры.

Nonfinite/экстремальные sigma проходят direct f64→f32 narrowing, а shader radius
переходит в i32 loop. Finite tiny sigma может underflow square. Это source-admission
gap; GPU behavior этих inputs не установлен. Первый шаг должен определить finite
admitted range/work budget и axis-zero identity, отказ до allocation/loop для
невозможного footprint. Новую logical-scale sigma semantics вынести отдельно;
crop scale test проверяет текущее поведение, не Skia-подобный scale blur.

## Следующая задача и критерии готовности

Предлагаемая задача: **«Сохранить bounded required-input footprint foreground
Blur/Compose и evolving support промежуточных проходов»**.

Изменения только engine filter IR, painter bounds planning, attachment/remap и
replay filter fold; architecture mapping decision и changelog fragment. Painting
contract менять только если исследование выявит необходимую cross-crate поправку,
тогда отдельный ADR. Не затрагивать decode cache, panic retirement, dependency
versions, workflows, новый backdrop или full textured DrawVertices.

1. Regression case сначала реально FAIL на baseline с ожидаемыми edge pixels.
   После fix restored narrow defects каждого из двух механизмов тоже FAIL;
   exact bytes finally восстановить, builds строго последовательно.
2. Одна existing family table, direct painter private readable seam и public
   Canvas/SceneBuilder producer. Small viewport 32×32 против crop 96×96 с
   integer shift (32,32); background, format, sampler и CTM одинаковы.
3. Cases: четыре края и четыре угла; fractional offset; sigma=(2,5),(5,2),
   (0,4),(4,0),(0,0); supported positive scale и DPR; clips снаружи и внутри
   filter; nested opacity/filter layers; Blur+Blur, Blur+Morph и identity chain;
   images/gradients/glyphs fallback. Outside total influence radius — exact blank.
4. Chain oracle дополнительно independent sequential convolution/nested equivalence
   внутри большого viewport. Source geometry reference не переиспользует ошибочный
   bounds predicate. Zero blur сравнить с unfiltered draw, не другим zero blur.
5. Для same-device crop начать с ≤1 LSB max-channel: equal pass count и integer
   grid обычно дают exact result. Если потребуется ≤2 LSB, записать locations,
   quantization/precision cause и проверить strict negative control. Existing
   CPU exp/quantization oracle допускает 3 LSB; это не автоматическое разрешение
   ослабить crop comparison. Ни threshold, ни snapshots не должны скрывать halo.
6. Assert bounded texture dimensions independent of дальних source coordinates;
   admission refusal сохраняет следующий healthy frame. По actual begun passes
   исключить незаявленные full-viewport intermediate в маленьком эффекте.
7. Измерить allocation identity high-water (включая idle/in-flight), actual pass
   counts, warmed CPU frame timings и GPU timestamps при наличии feature.
   Scene/device/backend/format, iterations, warmup и wait/readback policy записать.
   Не подменять physical GPU-memory prepared-resource ledger или driver AdapterRAM.
8. `cargo xtask check-changed` перед PR, required GPU suite с `FLUI_REQUIRE_GPU=1`,
   strict snapshot updates/forced success disabled. Native surface smoke и другие
   backend/platform paths явно обозначить executed/compiled/unavailable.

## Ограничения доказательств

Это subsystem audit, не workspace-wide modernization: optional platform features,
examples, profiler и все shader permutations не были полностью скомпилированы/
исполнены. Source review покрывает engine effect/resource/record/replay код,
production native/web routing, painting ownership и закреплённые upstream
контракты. Он не доказывает отсутствие других rendering ошибок.

## GPU-воспроизведение и измерения baseline

Исполнено на Windows, Rust 1.99.0, test profile `optimized + debuginfo`,
wgpu 30.0.1. Direct Device: **NVIDIA GeForce RTX 3070 Ti / DX12**, PCI
`0000:01:00.0`, driver `32.0.15.6094`. Оpaque white background,
RGBA8Unorm, single sample; source — black rect 4×4, позиции/sigma/scales
зафиксированы в [probe](engine-filter-footprint-probe.rs). Для nested opacity
alpha=0.5. Большая сцена имеет integer offset (32,32); comparison берёт
центральный crop 32×32 из 96×96. Headless отдельно запрашивает HighPerformance
device, его adapter identity в этом probe не экспортирована; GPU-метрики ниже
относятся к direct Device. Каждая headless small/large пара использует один owner.

| Случаи | Max-channel crop error direct / Scene replay, LSB | Результат |
|---|---|---|
| Left/right/top/bottom | 23 / 23 | FAIL |
| Top-left | 29 / 29 | FAIL |
| Другие углы | 30 / 30 | FAIL |
| Fractional offset | 28 / 28 | FAIL |
| Sigma=(4,2) | 47 / 47 | FAIL |
| Scale=(2,1.5), sigma=(4,2) | 46 / 46 | FAIL |
| Outer clip + nested opacity | 14 / 14 | FAIL |
| Compose Blur+Blur у края | 4 / 4 | FAIL |
| Clip внутри filter | 28 / 28 | FAIL |
| Sigma=(4,0) / (0,4) | 72 / 72; 58 / 58 | FAIL |
| Source вне радиуса влияния | 0 / 0, small exact white | PASS |
| Zero Blur | 0 / 0; direct filtered vs unfiltered max=0 | PASS |

Все 16 ненулевых footprint случаев падают даже при diagnostic threshold 3 LSB.
Это **18 строк**, 16 FAIL и два независимых контроля PASS; дополнительный
Compose-vs-nested support oracle имеет max error **35 LSB** и тоже FAIL.
Direct и Scene small images совпали точно; large images совпали точно,
кроме nested opacity с max=1 LSB. Это наблюдаемое основание для начального
≤1 LSB acceptance threshold в будущем fix. Для краёв одиночного Blur ошибка
23…72 LSB значительно превосходит numerical variation; chain edge 4 LSB
дополнительно подкреплён independent support witness 35 LSB.

Данные: полный лог (локальный `engine-filter-footprint-probe.log`),
[сводка direct-frame событий](engine-filter-footprint-measurements.json).
Счётчики временно вставлены перед фактическими `begin_render_pass` в reachable
effect/replay/clip paths и после новых pool allocations. Интервал direct
начинается до записи и завершается после readback/finish. Background clear,
target/staging allocation и readback не входят в pass counter engine encoding,
но входят в wall timing. Byte counters — TexturePool inventory при allocation,
с reusable idle и checked-out textures; это **не** peak всего GPU и не физический VRAM.
`null` в сводке означает отсутствие нового allocation event в этом интервале,
а не отсутствие resident storage.

| Сцена direct | Измеренный filter framebuffer | Engine encoding passes | Pool observation |
|---|---|---|---|
| Left, small 32×32 | origin (0,0), 32×32, source ошибочно viewport | 5: clear, geometry, H, V, composite | 12,288 B после трёх cold allocations |
| Left, large 96×96 | origin (19,35), 18×18 | 5 | Same painter уже имеет idle allocations; сумма inventory не isolated effect peak |
| Chain edge small / large | 32×32 / 33×33 | 7 | Два H/V фильтра, не новый backend |
| Nested clip/opacity small / large | 32×32 / 96×96 | 8 | Direct pool high-water за всю матрицу: **122,880 B** |
| Perf interior: rect (12,12,8,8), sigma=4, viewport32 | 22×22 | 5 | Warm iterations не создавали новых layer textures |

Warmed capture: один direct painter/device, **3 warmup + 10 samples**, каждый
sample encode→submit→wait/readback→finish, target и staging создаются на capture,
диагностический stderr включён. Последний run: **min 0.562 ms, median 0.638 ms,
max 1.004 ms**. Это baseline synchronous capture cost; не GPU-only duration,
не present latency, не throughput, не before/after speedup. First-use costs
excluded warmup, cold rows отдельно в логе. Host использовался другими сессиями;
10 samples недостаточно для p95/p99 claims.

Дополнительный Windows WDDM process sample: два опроса `GPU Process Memory`,
сумма matching `pid_38468_*` instances по physical adapters. На **1.238 s**
Dedicated Usage = **357,224,448 B** (~340.68 MiB), Shared Usage = **245,420,032 B**
(~234.05 MiB), Total Committed = **602,644,480 B** (~574.73 MiB). На 2.260 s
после завершения процесса counters = 0. Это **максимум двух samples** и lower
bound наблюдаемого process peak, не exact engine allocation high-water и не
доказательство resident physical VRAM всего процесса. Counter охватывает оба
direct/headless Device, driver/runtime caches и capture resources; он не может
приписать всю сумму одному filter. Из такой выборки нельзя заключать о leak
или нарушении prepared quota. Это иной показатель, чем 122,880 B layer pool.

Сохранены [samples](engine-filter-process-memory-samples.json) и
stderr процесса (локальный `engine-filter-memory-probe.stderr.log`). Для воспроизведения
после сборки probe запустить
[sample-engine-filter-process-memory.ps1](sample-engine-filter-process-memory.ps1).
Он выполняет готовый test binary в hidden process, без изменения исходников,
проверяет наличие witness, сохраняет process ID/время/counters. Sample interval
~1 s слишком груб для 1.3 s теста; exact peak остаётся непроверенным.

**Не измерено:** полный пик живых GPU allocations с retained/cache/glyph/external/
in-flight driver retention, физический peak VRAM, GPU timestamps, window-present
frame time. Resource accounting scope как раз не позволяет честно вывести их
из имеющейся квоты. Vulkan/Metal/WebGPU/GLES, browser/native window smoke,
affine/perspective blur и before/after исправление здесь не исполнялись.

### Воспроизведение и итоговая проверка

Из worktree root, без другого build этого worktree:

```powershell
& ./docs/research/run-engine-filter-footprint-probe.ps1
```

[Runner](run-engine-filter-footprint-probe.ps1) отказывает при dirty production
sources, сохраняет exact original bytes восьми своих диагностируемых файлов,
монтирует cfg(test) probe, использует `CARGO_BUILD_JOBS=1` и
`FLUI_REQUIRE_GPU=1`, ждёт завершения cargo, затем восстанавливает bytes в finally.
Production algorithms и API не исправляются. Expected baseline cargo exit **101**:
`baseline foreground crop/support defects: [left, ... y_only, chain_support]`.
No adapter skip, snapshot updates или forced success. Успешный будущий тест
потребует самого исправления, которое пока не выбрано пользователем.

После восстановления, existing suite:

```text
cargo test -p flui-engine --features testing --lib blur_filter_tests --locked -- --nocapture --test-threads=1
blur_oracle_match_within_3_lsb ... ok
image_filters_keep_nested_opacity_and_both_siblings ... ok
2 passed; 0 failed; 0 ignored
```

Existing-test log (локальный `engine-filter-existing-tests.log`) показывает, что прежние
blur tests проходят при доказанном viewport/chain дефекте; они проверяют другие
контракты. После diagnostic runs `git diff --exit-code -- crates Cargo.toml
Cargo.lock .github` возвращает 0: production sources/manifests/workflows без
изменений. `cargo xtask checks` — **PASS**: fmt, typos, taplo, docs-links/docs-paths,
workspace/reach/module-dag, toolchain/WGSL/globals и остальные source gates.
Gate log (локальный `engine-filter-audit-checks.log`) сохранён; финальные docs-links/docs-paths
также PASS (log (локальный `engine-filter-audit-final-docs.log`)). Это source/doc gate, не full
workspace compilation/execution. Archival research имеет exemptions некоторых
doc/marker проверок; 22 FLUI source URL paths также проверены вручную на наличие.
Ни `check-changed`, ни full CI, ни другие GPU backends не заявлены зелёными.

Сырые логи запусков остаются локальными и исключены через `.gitignore`. Проверяемые результаты приведены выше; скрипты воспроизведения и JSON samples включены в репозиторий.
