# Engine foundations: последовательность внедрения сейчас

Дата: 1 октября 2026. Это план атомарных изменений до 1.0, а не описание уже
выполненной миграции. Renderer единственный: wgpu. Альтернативный renderer,
Vello integration/CPU oracle и plugin backend не входят в план. Breaking changes
допустимы, когда исправляют контракт; временный deprecated API не оправдывает
сохранение некорректного ownership.

## Статус первой реализации

В рабочем дереве реализованы ordered typed runs для primitive families,
sealed handoff вложенных сегментов, immutable gradient/viewport parameters,
bounded recording arenas, общий DeviceDomain для managed submissions и
candidate/committed retained frames с двумя переиспользуемыми targets.
Result-ошибки промежуточных passes доходят до владельца кадра; quota refusal,
callbacks, конкурирующие panics и следующий кадр имеют executable проверки.
Решение закреплено [ADR-0100](../adr/ADR-0100-prepared-gpu-work-and-retained-frame-commit.md).

[Измерения](../research/engine-foundation-measurements.ru.md) содержат baseline,
стоимость правильного порядка, candidate copy/reuse и mutation failures.
Это завершение конкретного первого фундамента, а не всей программы ниже:
resource descriptor/lease snapshot, tagged color/linear working space,
managed producer capabilities, bounded diagnostic resolver и closing service
ещё требуют реализации. Текущие квоты явно исключают legacy caches/pools,
некоторые CPU intermediates и raw external allocations.

По измерениям приоритет следующей оптимизации — совместимый gradient
instance/shader, сохраняющий порядок linear/radial/sweep без отдельных pipeline
переключений. Затем solid/gradient compatibility и более широкие workloads.
Сортировка разных семейств, возвращающая старую ошибку, исключена.

## Исходная точка и доказательства

[Аудит возможностей](../research/engine-capability-audit.ru.md) фиксирует пять
исправлений: owning Image cache key, размеры content key, ordered ImageFilter
items, удаление alpha cutoff и adapter-gated feature selection. Там приведены
GPU readbacks и mutation runs, а также точные ignored logs. Их green baseline
сохранить перед миграцией; документ не объявляет непротестированные команды
корректными. `embedded_gpu_scene` уже демонстрирует same-device producer pass,
external texture и foreground UI; `ai_streaming` — scheduler/service IO streaming.
Их screenshot/fixture проверяет сценарий, не универсальность GPU или качество LLM.

[ARCHITECTURE](../../crates/flui-engine/ARCHITECTURE.md) закрепляет pure wgpu и
GPU-free RasterBackend test seam. Статусы ADR нельзя подменять заголовками:
ADR-0087 принят только в §3/§4 (damage/retained target), §1/§2/§5 Proposed;
его CPU/backend предложение здесь не принимается. ADR-0098 Proposed при уже
идущей миграции owned f64 geometry; ADR-0088 Proposed с частично выполненными
SDK moves. ADR-0095 Accepted in part включает wire vocabulary, owner inbox и
local devtools endpoint. Изменение принятых cross-crate частей требует нового
superseding ADR; Proposed части уточняются до принятия, не считаются реализованными.

## Обязательные поправки после adversarial review

[Решения review](engine-plan-review.ru.md) имеют приоритет над первоначальной
декомпозицией ниже. Три независимых проверки дали 24 замечания с повторяющимися
сценариями: [correctness](engine-plan-review-correctness.ru.md),
[recovery](engine-plan-review-recovery.ru.md), [API](engine-plan-review-api.ru.md).
Это проверка плана и текущего кода, не доказательство прогноза 2031.

1. До implementation снять warm/cold baseline для solid, alternating gradients,
   nested clips/effects, image churn и text atlas pressure. Одинаковые adapter,
   build/profile, scenes и sample count; CPU record/prepare/encode p50/p99,
   GPU timestamps при поддержке, allocation/upload/pass counts и peak live+retired
   payload. Ledger не измеряет driver VRAM. Численные regression bounds принять
   по результатам baseline до приёмки PR, не выдумывать скорость заранее.
2. Первая поставка атомарна: все ordered families, recorder/seal, nested consumers
   и удаление Phase/category replay. Локальные commits можно разбить; dual ordering
   representation не выпускать. Immutable параметры включают viewport/target
   uniforms, не только stops: inventory всех queue writes перед одним submit.
   Run scoped одним target {origin, viewport, DPR, generation}; полный state key.
   Stops >8 не silently clamp: checked supported limit и explicit rejection.
3. Минимальный admission/reservation входит в первую поставку, до recorder growth,
   conversion temporaries и GPU allocations: CPU peak bytes, runs/stops/pass/
   bindgroup counts, staging, GPU payload, submitted bytes/frames. CPU finish
   не GPU completion. Один submit owner удерживает reservations до completion
   либо teardown device generation; wgpu retention обеспечивает safety, не cap.
   Refusal сохраняет предыдущий complete target и допускает следующий малый frame.
   try_reserve не обещает containment любого OOM/process abort.
4. SourceScene с TextureId разрешает latest allocation до lowering. Allocation
   snapshot начинается при lowered recording/seal; rebind до lowering и после
   seal имеют разные тесты. Managed allocation и trusted raw import различаются:
   Arc lease не предотвращает raw clone.destroy, token не доказывает provenance,
   внешние allocations не покрываются managed cap.
5. Allocation identity не content snapshot. LatestAtOrderedSubmission требует
   одного submit owner; snapshot требует копии/allocation и dependency/budget.
   Producer commit меняет content revision, invalidates dependent regions и будит
   realm при unchanged Scene. Per-draw sampling authoritative; resource default
   только explicit mode. Premul opacity масштабирует RGB и alpha; source encoding
   и working representation не взаимозаменяемы.
6. Effects различают input, expanded output, backdrop read и write clip regions.
   Unknown/nonlocal dependency вызывает full repaint. Partial сравнивается с fresh
   full по всему target, включая halo вне начального damage. Window/capture имеют
   один SourceScene и target descriptor; DPR 1/1.5/2, fractional geometry и negative
   origin обязательны. Validation после f64→f32/DPR, conditioning и crossing w=0
   policy обязательны: finite input и inverse Option сами этого не гарантируют.
7. Color split на две coherent поставки: source intent + validated serde/hash +
   named legacy SDR conversion во всех consumers; затем целиком linear premul
   composition/effects/output. Не смешивать encoded/linear pipelines в frame.
   Domains color matrix/advanced blend явно закрепить; RGBA16F не обещает HDR display.
8. Managed GPU API получает одного diagnostics/submit owner. Generation-aware
   fault mailbox не приписывает unattributed error последнему frame. Completion
   не scanout. Internal Drops infallible/idempotent, без user callbacks; rollback
   до submit и retirement после него различаются. First failure authoritative;
   double failures и next valid frame входят в acceptance.
9. Cancellation cooperative для bounded yielding work: отдельно admission,
   checkpoints/deadlines, stale-generation publication, worker completion и GPU
   retirement. Stop UI не доказывает disconnect/освобождение ресурсов.
10. Private preserved IR invariant закрепляется Mapping decision. Изменение
    accepted ADR получает новый Supersedes и старый Superseded-by; Proposed части
    уточняются до acceptance. glamx не добавлять без actual conditioning consumer.

## Уточнения после повторного независимого review

[Второй review](engine-plan-rereview.ru.md) проверил закрытие прежних замечаний и
обнаружил дополнительные границы. Следующие решения уточняют требования выше:

- Opaque consuming PreparedSubmission связывает command buffers, charges,
  DeviceDomain/generation и target identity; private constructor, submit/discard
  потребляют одно value. Нельзя вручную сопоставить чужие buffers и reservation.
- Владелец охватывает **все** production submissions: clear, backdrop flush,
  blur/mask/blit/advanced blend и final. Несколько submissions допускаются;
  PartiallySubmitted retires уже отправленное, discard только остаток. Наивное
  объединение изменяет queue-write ordering: effect/viewport uniforms сначала
  freeze per encoded use. Dispatcher render error распространяется, не log+copy.
- Last committed image защищён отдельным candidate/staging target, не только
  отсутствием present. Его allocation и partial-copy charged; known frame work
  preflight до первого submit. Поздняя ошибка не откатывает GPU: candidate
  discarded/invalidate, submitted ресурсы retire. Promote после соответствующих
  completion/owned validation outcomes; unknown fault делает generation uncertain.
  Такая commit discipline требует явного пересмотра accepted retained-target ADR.
- Managed painters на одном GPU делят instance-owned DeviceDomain: submit,
  diagnostics и retirement owner один; realm quota/wake остаются отдельными.
  Никакого process-global registry. Independent raw Device clones вне domain
  trusted и не дают обещания общего device cap.
- Первый PreparedIrBudget — quota новых tables/uniforms/instances и их CPU
  temporaries, **не** whole-engine memory cap. Legacy cache/atlas/effect allocations
  перечисляются как exclusions с telemetry/отдельными limits. Whole-ledger
  delivery снимает exclusions; allocation transfer не double-charge и не release.
- Retirement service DeviceDomain переживает окна: Closing/Draining/Lost/Retired,
  bounded nonblocking native poll и browser progress. Callback только mailbox,
  без strong owner cycle/user Drop. Deadline не completion и не VRAM-release proof.
- Wgpu30 ErrorScopeGuard thread-local и !Send: explicit pop в обратном порядке
  на owning thread даже при early error, edge получает future без guard. Captured
  fault вторичный к primary error; other-thread fault generation-wide uncertain.
- Raw &Device/&Queue — trusted embedder API: reference можно clone/destroy/submit.
  Managed producer использует constrained FLUI operations без Deref/raw authority;
  scope lifetime сам по себе не security boundary. Оба режима остаются pure wgpu.

## Порядок поставок с отдельными границами приёмки

### 1. Заморозить ordered GPU command IR

**Владелец: flui-engine.** Изменить `command_ir.rs`, `batches/`, `painter/`,
`replay/`: mutable recorder превращается в sealed DrawSegment с ordered DrawRun,
типизированными ranges в instance arenas и immutable gradient stops.
Первые локальные red tests: gradient→solid и два gradients с разными stop tables.
В той же поставке перенести все primitive families в единственный ordered stream
и immutable seal. Batch coalescing только соседних совместимых runs; глобальная
сортировка по primitive type запрещена конструкцией replay.

Sealed segment содержит immutable stop allocation/binding; submit owner удерживает
accounting до GPU completion, что отдельно от CPU frame finish;
для начала отдельный STORAGE buffer на segment. `UniformPool` UNIFORM-only
не подходит. Reuse вводится после correctness и измерения, не повторяет shared
queue.write_buffer offset-zero overwrite. Constructors/ranges private; публичный
API не получает arena indexes, mutable scene или новый backend seam.

**Готово:** production replay всех families читает ordered stream; старые parallel
category replay paths удалены. Red-gradient→blue-solid overlap и два stop tables
дают правильные pixels при одном queue.submit и падают при возврате сортировки/
shared-stop overwrite. Existing nested filter/opacity/clear tests остаются green.
Private IR не требует нового pub item. Для retirement public embedder может
потребоваться breaking engine-owned submission hook: он входит в эту же поставку
и сразу используется Renderer/headless/embedder, не откладывается до SDK этапа.
Manual void finish_frame не подтверждает submission/discard/completion outcome.

### 2. Исправить resource descriptor и frozen allocation lifetime

**Владелец: flui-engine; зависит от sealed IR.** `external_texture_registry.rs`,
`texture_cache.rs`, `resources.rs` и image replay должны переносить explicit
size/format/sampling/alpha policy. Registry сохраняет выбранный sampler;
update сохраняет policy и проверяет совместимость до view/bindgroup creation.
На первом этапе поддерживается реальный encoded-sRGB путь; неподдерживаемый
space возвращает ошибку, не silently трактуется как sRGB.

Следующий атомарный hunk: allocation generation и frame-local strong leases
в lowered IR. unregister запрещает будущую resolution, но ранее sealed frame
держит allocation. Freeze allocation не обещает freeze texels: dynamic producer
обновляет content по явному synchronization contract. Update/rebind не должен
подменять resource уже записанного frame.

**Готово:** production image/external replay читает captured lease, не поздний
bare ID lookup. Record→update/unregister→submit показывает исходную allocation;
следующий frame видит replacement/absence. Descriptor rejection сохраняет старую
entry и следующий valid update работает. Device recreation инвалидирует старый
owner generation; cache churn измеряет retained CPU+GPU bytes и release after
retirement. raw wgpu Texture не доказывает same-device provenance: managed factory
или explicit trusted owner + wgpu validation, без ложной introspection guarantee.
Scene-level lease/TextureId contract меняется отдельным ADR и consumer migration.

### 3. Привести clip и effects к одной ordered композиции

**Владелец: flui-engine; зависит от 1/2.** `state_stack.rs`, layer traversal,
`layer_offscreen.rs`, `offscreen/`, replay: immutable clip dependency chain,
содержащая shape, transform, fill rule и intersection/difference. Scissor лишь
консервативное bounds ускорение; rotated rect, path и nested rounded clips
выполняются через точную mask/stencil композицию на wgpu.

Использовать существующие kurbo/lyon, device-space tolerance и owned f64 inputs.
Любые nonfinite/singular/projective значения получают deliberate validation policy
до allocation/upload. Finite constructors внутри lowered IR делают invalid GPU
geometry непредставимой; ошибка одного draw не должна оставлять clip/effect stack
повреждённым. Если операция не поддержана, typed availability/error сохраняет
правду вместо bounds approximation с тем же именем.

Window/headless использует один visitor для BackdropFilter, ShaderMask и
Leader/Follower; offscreen captures сохраняют всю последовательность и trailing
segment. Эффекты объявляют source/destination dependency и region, не подменяют
групповую opacity per-child alpha.

**Готово:** nested clips holes/difference, rotated rectangle, sibling после restore,
empty clip, group opacity, nested filter и headless/window-equivalent capture
имеют discriminating interior/edge readbacks. Partial damage straddle compare
с full frame, включая destination-replacing blends; known approximations удалены
или explicit unavailable. Не требуется bit-identical AA на всех GPU.

### 4. Внедрить color foundation через реальный gradient/image путь

**Владельцы: flui-painting → flui-layer → flui-engine; зависит от 1/2/3.**
[Color proposal](../research/color-foundation-adoption.ru.md) определяет собственный
public FLUI source-space tagged finite float color; внутреннюю математику выполняет
Linebender color 0.3.3. Linear premultiplied working representation отделена от
source values и presentation mapping; >1 headroom не clamp до presentation.
Сначала source intent с named legacy SDR conversion, затем единый linear working
transition во всех consumers: две поставки, без утверждения HDR readiness.

Typed space/alpha metadata проходят Paint→Shader→Layer→IR→WGSL; gradient ramp
подготавливается color Interpolator/gradient, image alpha учитывается sampling/
blend pipeline. Default Oklab/premul и compatibility behavior ADR-0098 необходимо
явно определить; ADR-0098 Proposed уточняется до acceptance. Изменение другого
accepted decision требует superseding ADR; не просто поменять lerp.

**Готово:** public consumer показывает actual ramp; transparent saturated endpoint,
Oklab vs linear midpoint, P3 source conversion, zero alpha и finite >1 storage
сверяются с independent fixtures/GPU readback. Migration удаляет duplicate hand
conversion formulas; public Color API всех production paint consumers перенесён.
HDR output только после Rgba16Float pass-chain/readback + host negotiation tests.
half 2.7.1 подключается лишь к такому consumer; сохранённый HDR intent сам по себе
не доказывает HDR display.

### 5. Закрепить host GPU producer и SDK capability contract

**Владельцы: flui-engine/flui-app/flui-sdk; зависит от resource lease contract.**
Развить существующий `embedded_gpu_scene`, не новый game renderer. Trusted producer
может получать raw device/queue; managed producer получает constrained operations
без raw authority и output capability с format/size/generation/availability,
имеет explicit submit ownership, resize/device-loss и retirement. UI render pass
не передаёт producer arbitrary internal encoder/state. Frame begin/finish/error
cleanup определяет один owner; forgotten obligations выражены RAII/typed state.

SDK экспортируется только при реальном package/app caller и с surface pinning.
Input/focus/semantics принадлежат realm и host; 3D pixels не создают accessibility.
Presentation accepted/committed/displayed различается; device loss и suspend
возвращают typed availability, не panic и не fake presented acknowledgment.

**Готово:** same-device production embedder, resize, unregister while in flight,
producer error→next frame, device recreation и minimize/restore smoke. Публичный
пример и SDK package используют именно production capability path; unwired pub
hooks удалены. Native smoke требуется отдельно от headless pixel test.

### 6. Закрепить runtime AI/test contracts на существующих инструментах

**Владельцы: service/runtime/devtools/protocol, не raster engine.** Реальный
provider adapter работает на IO edge и доставляет results в next frame без async
в build/layout/paint. Сравнить typed provider library rig-core с текущим reqwest
decoder на одинаковом owned loopback SSE fixture прежде чем менять dependency.
Generation replacement, bounded body/queue, cancellation и redacted error contract
принадлежат host; crate не выбирает authority и secret handling за FLUI.

Развивать существующие flui-protocol, authenticated local devtools и
`tools/desktop-mcp`, а не параллельный testing framework. Semantic selectors+
pixel oracle, stale target и enabled/action revalidation, accepted/committed/
displayed wait, capture permissions и artifacts связаны одним scenario record.
Virtual clock применяется к headless scheduling; native host time не объявляется
детерминированным.

**Готово:** local SSE controlled success с разными request ordinals, replace,
Stop без поздних UI updates, 503/malformed/oversize/recovery; disconnect лишь если
server действительно наблюдал его. Existing desktop-mcp CLI сценарий создаёт PNG
и redacted diagnostics, управляет только owned demo processes и очищает их finally.
No credentials/external billing; это transport/lifecycle test, не LLM quality eval.

## Что брать из ecosystem сейчас

[Ecosystem memo](../research/wgpu-ecosystem-adoption.ru.md) содержит registry/API
доказательства. color — первый новый foundation с production gradient consumer.
Kurbo, glam, lyon, etagere, smallvec, Parley, Naga/naga_oil/wgsl_bindgen и profiler
уже используются: расширяем их production contract, не добавляем дубль.
Glamx 0.3.1 допускается только при actual checked-inverse/SVD consumer: glam ^0.33.7,
MatExt::try_inverse и DSvd2 полезны для validated inverse/tolerance; rigid Pose
не общий affine transform. Conditioning threshold определяет FLUI и independent
near-singular tests. Публичные f64/unit geometry не заменяются glam types.
Encase pilot проверяет реальный storage stride, но текущий layout уже правильный;
новая dependency не нужна только ради успешно прошедшего игрушечного теста.

## Горизонт 2031 выражается в сегодняшних инвариантах

| Долговременное требование | Миграция сейчас | Не обещаем без отдельного proof |
|---|---|---|
| Wide gamut/HDR | source-space/alpha intent + output descriptor, PR4 | HDR monitor/platform pipeline |
| Games/3D/custom GPU content | allocation leases, producer capability, PR2/5 | universal engine/plugin backend |
| Agent-mediated/adaptive UI | existing typed semantic actions/authority/frame waits, PR6 | permissions из pixel labels или global IDs |
| Large scenes/mobile memory | ordered adjacent batching, bounded retirement/budgets, PR1/2 | скорость без profiler/workload |
| Export/capture/remote | immutable frame resource ownership, redacted capture artifacts | arbitrary scene replay/fonts distribution |
| Robust recovery | generation+typed availability+error cleanup, PR2/3/5/6 | correctness по одному enum или signature |

Каждый PR проходит existing targeted public/readback families, mutation check для
исправляемой ошибки, `cargo xtask check-changed` и self-review против main. Не
создавать новый gate без существующего merge path. Одновременно компилирует один
worker. Проверки не выполнялись при написании плана. Полная миграция закончена
только когда каждый новый module/API достижим из production caller, прежний wrong
path удалён, ADR согласован, и failure→next operation сохраняет progress.
