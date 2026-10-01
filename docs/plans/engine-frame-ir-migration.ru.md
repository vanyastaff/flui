# Неизменяемый упорядоченный frame IR: атомарная миграция engine

Дата: 2026-10-01. Статус: проект реализации после зелёного текущего baseline;
это не принятый ADR и не отчёт о реализованном поведении.

## Наблюдаемая причина

`command_ir.rs::DrawSegment` хранит раздельные массивы rect/circle/arc/shadow,
трёх gradient kinds, tessellation, cached/external images и glyphs.
`replay/flush.rs::flush_segment` обходит их по категории, а не в порядке записи.
`Phase`/`last_phase`/`would_reorder` компенсируют часть перестановок разбиением
segment. Gradient сознательно исключён: `pipeline_set.rs::refresh_gradient_bind_group`
пишет stop tables каждого segment через `queue.write_buffer` в один buffer,
в offset 0. Все записи перед одним submit завершаются до GPU draw: два bindgroup,
ссылающиеся на этот buffer, читают последнюю таблицу. Это lifetime-дефект,
не только неправильная сортировка. Комментарии прямо закрепляют обход дефекта.

Публичное воспроизведение порядка: Canvas рисует красный shader-filled rect,
затем синий solid rect поверх него. Sample в пересечении должен быть синим;
fixed category replay рисует solid до gradient и оставляет красный.
Публичное воспроизведение таблиц: первый gradient слева красный, второй справа
зелёный; между ними opacity/effect boundary создаёт два sealed segments в одном
frame. Первый должен оставаться красным; shared offset 0 делает его зелёным.
Для диагностики достаточно constant-color двух stops: это исключает спор о
пространстве интерполяции. Ни один test не должен воспроизводить сортировку
категорий внутри oracle.

## Решение до изменения публичных контрактов

Сохранить существующие `Scene`, `DisplayList`, wgpu, `DrawBatcher`, `GpuReplay`
и синхронный frame path. Это изменение приватного engine lowering, не новая
Scene сериализация и не универсальный raster backend. ADR-0006 уже принимает
явный IR; сохранение этого accepted инварианта при приватном улучшении
record/replay фиксируется Mapping decision, а не переписыванием accepted ADR. ADR-0087 §1/2 остаётся Proposed: этот шаг не
переносит lowering в другую crate и не вводит software renderer.

Концептуальные приватные типы (имена могут измениться при реализации):

```rust
struct SegmentRecorder {
    runs: Vec<DrawRun>,
    // Mutable typed arenas: instances, vertices, indices, image descriptors.
    stops: Vec<GradientStop>,
}
struct DrawSegment {
    runs: Box<[DrawRun]>,
    // Sealed typed arenas, owned by this segment.
    stops: Box<[GradientStop]>,
}
enum DrawRun {
    Rect(RectRun), Circle(CircleRun), Arc(ArcRun), Shadow(ShadowRun),
    LinearGradient(LinearRun), RadialGradient(RadialRun), SweepGradient(SweepRun),
    Tess(TessRun), CachedImage(CachedImageRun), ExternalImage(ExternalImageRun),
    Glyph(GlyphRun),
}
```

Это описание формы, а не новый `pub` surface. Каждый run содержит диапазон
в своём typed arena и зафиксированное состояние: pipeline/blend, scissor,
clip. Gradient run дополнительно обращается к stop table своего segment;
image run — к уже определённой resource-resolution policy. Range существует
только для соответствующего варианта, constructor внутри recorder проверяет
bounds и ненулевую длину. Replay принимает `&DrawSegment`; `clear()` в replay
исчезает. `seal(self)` забирает recorder, данные не могут дописываться после
планирования GPU-проходов. Трансформации по текущему контракту записываются
в instance/vertex payload; эта миграция не должна незаметно менять координаты.

Runs перечисляются строго в painter order. Merge допускается исключительно
с последним run того же типа при непрерывном диапазоне и полном равенстве
state/resource keys. Нельзя собрать все Rect в один pass, перепрыгнув через
Gradient. Pass fusion — следствие соседства и совместимости, а не причина
перестановки. AdvancedShape, OpacityLayer, Filter, OffscreenTexture остаются
упорядоченными `DrawItem` barriers. Каждый вложенный segment obeys тот же
контракт, включая shader-mask/backdrop/blur inputs.

## Stop tables и GPU lifetime

Самый маленький безопасный первый implementation — отдельный immutable
`STORAGE` buffer на sealed table через существующий `wgpu::util::DeviceExt`
`create_buffer_init`, отдельный bindgroup на этот buffer. Table создаётся
один раз при подготовке segment и сохраняется до кодирования всех draws,
которые на неё ссылаются; закодированные wgpu commands удерживают ресурсы до
исполнения. Прежний global buffer + write offset 0 удалить, не оставить как
fast path. Empty table не создаёт binding. Checked byte length должен отвечать
storage binding limit; переполнение возвращает ошибку кадра, не silently drop.

`UniformPool` уже решает уникальную allocation на использование и reset
только после финального flush. Но он выделяет `UNIFORM | COPY_DST`: gradient
shader использует STORAGE, поэтому прямое повторное использование этого pool
некорректно. Не копировать второй pool сейчас ради оптимизации. Следующий
измеряемый шаг может расширить существующий resource allocation policy usage
kind и frame ownership либо собрать все tables кадра в одну immutable
storage arena с offsets. Для этого одновременно меняются shader index contract,
GPU limits и bounds validation; это отдельная оптимизация после correctness.
`wgsl_bindgen` сохраняется для shader ABI там, где он уже используется;
новый вручную несовместимый stop layout не нужен.

Queue lifetime: нельзя повторно писать в allocation, пока новые draws ещё
могут попасть в тот же submission после предыдущих draws. Переиспользование
между последовательно отправленными кадрами подчиняется queue ordering;
несколько encoder flush одного кадра не означает finish_frame.
Public painter, window Renderer и capture используют один sealed IR/replay.
Первый delivery мигрирует все production submission sites, включая early clear,
backdrop flush, blur/mask/blit и advanced blend, не только final renderer submit.
Множество submissions допустимо: owner сериализует каждый и переносит его
reservations в retirement; наивное объединение всех encoders в один запрещено,
пока per-use parameters и dependency ordering не frozen.

Концептуальный consuming opaque `PreparedSubmission` связывает command buffers,
charged reservations, device domain/epoch и target identity. Constructors private;
нельзя передать owner произвольный raw CommandBuffer как якобы charged frame.
Owner consumes bundle, сам submits и регистрирует completion. Public embedder
получает только реально используемый production handoff; void finish_frame
заменяется/мигрирует совместно с Renderer/headless/example callers. Trusted raw
queue.submit вне owner не получает managed quota/retirement guarantee.

Перед выбором Rust signature составить inventory каждого production submit:
producer inputs, targets, mutable writes, reuse assumption, error path, retirement
owner. Frame state различает Prepared, PartiallySubmitted, Submitted, Discarded;
после частичного submit отправленные bundles retire по completion, discard
освобождает только ещё неотправленные. Async device faults маркируют epoch
uncertain, не обещают точный frame attribution или transactional GPU rollback.

## Граница snapshot и target state

SourceScene сохраняет текущую resource-reference семантику до начала lowering;
создание Scene не является snapshot GPU allocations или texels. Allocation
захватывается при lowering/record и принадлежит sealed lowered frame. Смена
allocation после record не меняет его lease, но texels той же allocation могут
быть volatile согласно явному producer synchronization contract. Эти две
политики проверяются отдельно; source Scene не получает wgpu handles.
Content revision/damage/wake owner согласуется с resource migration до первой
поставки: allocation identity не сообщает, что producer изменил пиксели.

Segment/prepared target фиксирует logical extent, DPR, physical extent,
device-to-target origin и viewport epoch. Эти значения берутся из одного
validated target descriptor, а не вычисляются независимо window/capture.
Run merge не пересекает target context. Complete per-run compatibility key
содержит pipeline/alpha mode, blend, immutable clip reference, scissor,
effective sampler и allocation identity, а для gradients — kind/table identity
там, где она не invariant всего segment. Float transforms сравниваются точно;
приблизительное равенство ради merge не допускается.

Immutable per encoded use относится ко всем GPU inputs: stop tables,
viewport/target uniforms, clip/effect parameters, а не только к CPU arrays.
Shared viewport buffer сейчас обновляется `resize` до queue.submit; два flush
с resize между ними могут дать первому draw параметры второго. Первая поставка
создаёт frozen viewport/target bindings per prepared target; resize меняет
только следующие prepared targets. Нельзя возвращать старый shared-offset
path как optimization. Inventory queue.write_buffer должен доказать уникальные
slots для каждого одновременно encoded use, включая offscreen passes.

## Admission, submission и failure ownership

Первая поставка вводит explicit `PreparedIrBudget`: quota новых IR данных,
prepared bindings и submissions, а не whole-device memory cap. Cache/atlas/pool,
старые и внешние allocations до whole-ledger milestone не покрываются этой
гарантией; это отражается в diagnostics и публичных утверждениях.
Quota имеет checked caps на runs/segments, instances,
stop count/bytes, prepared passes и transient prepared GPU bytes. Reservation
предшествует GPU buffer/bindgroup allocation и CPU expansion. Один приватный
frame-preparation owner владеет reservation, подготовленными bindings и leases;
discard/error возвращает unsubmitted capacity. После submit reservation
передаётся existing submission owner и освобождается по completion либо
доказанному device teardown. Завершение CPU encode не означает GPU retirement.
Нельзя ввести учёт, у которого нет производственного completion owner.
Подготовить и reserve известные требования всего frame до первого submit;
отказ после частичной отправки не освобождает уже submitted reservations.

Stops не обрезаются молча до 8: весь validated input сохраняется и исполняется,
либо unsupported count/limit возвращает typed error до частичной записи frame.
Checked arithmetic учитывает storage byte length, shader indexing и device
max binding size. До реализации выбрана одна deliberate policy: поддерживать
полную таблицу в storage shader; cap определяется admission/device limits,
не прежним implementation clamp. Если shader пока способен только на 8 stops,
он обязан явно reject большее число до удаления ограничения, а не менять ramp.

Last committed image никогда не используется как writable target failed frame.
Frame пишет в reserved candidate/staging target; partial frame получает копию
last committed content и дополнительную reservation на target/copy work. Только
успешный frame commit меняет committed identity; ранний clear/backdrop submit
может менять candidate, но не прежние complete pixels. Present/async fault
uncertainty классифицируется отдельно: completion не доказывает scanout.

Если поздний admission/encode failure случился после раннего submit, candidate
не commits/presents как успешный frame, damage остаётся owed, submitted chunks
и их target allocations сохраняют charges до completion. Оставшаяся unsubmitted
работа discarded. Dispatcher/effects обязаны propagate first error к frame
owner: log-and-continue после failed painter.render перед backdrop copy недопустим.
Inventory таких swallowed-error/skip branches входит в первый delivery.
Cleanup error не подменяет first error; следующий valid frame реально рисуется.

Каждый viewport/target/effect uniform frozen per encoded use до любой попытки
merge/reorder submissions. Текущий blur slot reuse обоснован own render_blur
submit: его нельзя сохранить, просто перенаправив submit в final owner.
Два blur invocations разных sizes/radii и down/up passes получают независимые
bindings, пока их commands одновременно encoded. Producer→copy→blur→composite
dependencies сохраняют GPU order; CPU queue.write_buffer не является command
между произвольными draws и не создаёт pixel snapshot.

## Атомарная последовательность реализации

Один delivery boundary включает все primitive families ordered/sealed IR,
immutable bindings per encoded use и минимальный admission/retirement owner.
Локальные commits и временные рабочие adapters допустимы для разработки;
финальная production поставка не содержит dual order representation.

1. Зафиксировать baseline failing readbacks gradient-before-solid и two-table
   segments через публичные SceneBuilder/Canvas и HeadlessRenderer. Добавить
   ordered-pair family cases и repeated gradient kinds, чтобы не сузить proof.
2. Совместно изменить все producers, typed ordered ranges и replay consumers,
   включая nested offscreen. Удалить Phase heuristic/category scheduling,
   разделить mutable SegmentRecorder и sealed DrawSegment в том же delivery.
3. Удалить shared stop writes; создать immutable table bindings и frozen
   viewport/target/effect inputs. Добавить admission before allocation,
   consuming PreparedSubmission handoff для каждого submit, frozen effect slots,
   candidate-target transaction и capacity reuse после failure.
4. Проверить production window/capture shared lowering facts, публичный cube
   capture, recovery matrix и mutation regressions. Foundation считается
   выполненным только после удаления прежних wrong paths во всех families.
5. Затем transformed images отдельным atomic shader/vertex contract: настоящий
   transformed quad либо explicit refusal. Это не оправдывает оставление
   category replay и не смешивается с stop-table proof.

Delivery не обещает linear-color, exact clip chain или managed GPU widgets;
их prerequisites — immutable ordering/resources/target context — уже действуют.
Correctness-first отдельные stop allocations измеряются на contiguous и
alternating workloads; reuse/frozen frame arena оптимизируются после proof,
но admission cap действует с первого релиза.

## Файлы и проверяемые последствия

| Файлы | Изменение | Сохраняемая граница |
|---|---|---|
| `command_ir.rs` | recorder/sealed segment, typed runs | приватный IR |
| `batches/{shapes,gradients,paths,images,text}.rs` | запись run после успешной записи данных | существующий public painter |
| `painter/mod.rs`, `painter/layer.rs`, `layer_compositor.rs` | seal и ownership offscreen segments | существующие save/restore semantics |
| `replay/{mod,flush}.rs`, `layer_offscreen.rs` | обход ordered runs, immutable data | один replay для window/capture |
| `pipeline_set.rs`, `effects_pipeline.rs` | убрать mutable shared table, table binding | существующие gradient shaders/layout |
| `resources.rs`, submission owner, frame protocol/retained target | PreparedIrBudget, consuming bundles, candidate/commit/retirement | без нового global state; не whole-device cap |
| `gradient_blend_readback_tests.rs` и существующие таблицы pixel contracts | новые rows поведения | public API, один test binary |
| `ARCHITECTURE.md` Mapping decision; superseding ADR только при изменении accepted контракта | реальные инварианты | не декларация о другом backend |

## Regression matrix

| Вход | Sample/ожидание | Что отличает от дефектного кода |
|---|---|---|
| red constant linear gradient [0,0,48,48], затем blue solid [16,16,64,64] | (24,24) blue, (8,8) red | fixed category order делает overlap red |
| тот же вход с reversed draw calls | overlap red | тест не навязывает категории |
| red linear, blue solid, green radial на одной области | центр green; uncovered red/blue | три последовательных runs |
| red constant gradient left, opacity boundary, green gradient right | left red, right green | shared stop buffer перекрашивает left |
| linear/radial/sweep mixed с разными stops и двумя effect boundaries | отдельно заданные constant-color sample points | не только один gradient kind |
| два gradient inputs внутри nested Blur/Opacity, с siblings до/после | каждый input color и sibling сохраняются | offscreen использует тот же IR |
| image, затем solid; solid, затем image; glyph между ними | overlap соответствует последнему draw | ordered typed ranges для остальных kinds |
| clip A, gradient, clip B, gradient | pixels за каждым clip отсутствуют | merge не теряет state boundary |
| два кадра, новые stop values при тех же размерах | второй кадр показывает новые цвета, первый readback неизменен | allocation reuse/immutable seal |
| больше 8 stops с различимым девятым endpoint | весь ramp либо explicit count error | silent clamp никогда не green |
| два viewport sizes/target origins между flushes до одного submit | каждый draw использует frozen собственные параметры | shared viewport overwrite |
| SourceScene build→rebind→lower; record→rebind→encode | первая sequence latest allocation, вторая captured allocation | source и lowered snapshot не путаются |
| same allocation content update | documented texels + producer damage/wake | lease не выдаётся за pixel snapshot |
| DPR1/1.5/2, fractional origin и offscreen rebase | одинаковый Scene/descriptor window и capture | device-only demo не ложный parity proof |
| два разных blur sizes/radii в одном encoded interval | каждый совпадает со fresh GPU reference | frozen slots, не last queue write |
| producer pass→backdrop copy→blur→composite | current frame texels читаются в dependency order | hidden early submit не обгоняет producer |
| отказ после раннего clear/backdrop submit | last committed whole-target pixels прежние; candidate не commits | early GPU write не маскируется discard final encoder |
| PartiallySubmitted и delayed completion | sent charges остаются, unsent возвращаются | submitted work не double release |
| painter.render error именно в backdrop dispatcher | first error до copy/submit, следующий backdrop valid | log-and-continue устранён |
| admission отказ, encoding отказ, cleanup отказ, затем valid frame | first error authoritative, capacity возвращается, damage owed | нет partial present/утечки |
| submitted frame завершён после следующего prepare | retirement освобождает лишь completed allocation | CPU finish не считается completion |
| больше device storage limit / integer overflow | typed frame error, следующий валидный кадр работает | нет silent clamp/drop/poison |

Concrete table runners и helper constructors брать из существующего тестового
семейства; не создавать новый cargo-spawning binary. Run appropriate engine
readbacks и public cube capture после implementation. GPU adapter/backend и
sample results входят в evidence. Revert stop-buffer fix и order fix отдельно:
соответствующий row обязан падать при каждом revert.

## ADR draft для обсуждения

На момент первого чтения `rg --files docs/adr | rg 0100` не нашёл ADR-0100;
номер не зарезервирован, перед созданием нужен повторный inventory. Private
ordered/sealed IR, сохраняющий ADR-0006 record/replay invariant, фиксируется
Mapping decision и regression tests. Accepted ADR не редактируется молча.
Если новый contract изменяет SourceScene snapshot identity, external-resource
lifetime или flui-layer ownership, требуется Proposed superseding ADR с
`Supersedes` и обратным `Superseded-by` при принятии; migration callers и
production wiring идут вместе. Proposed части ADR-0087 здесь не принимаются.

## Цена пропуска

Без этого foundation order зависит от вида примитива, а shader resources —
от последней CPU write перед submit. Каждый новый primitive/effect увеличивает
число комбинационных исключений. Damage, cached subtrees, concurrent preparation
и external GPU content затем потребуют обходов либо переписывания IR. Сейчас
breaking private structure дешёва; типизированные ordered runs устраняют саму
причину, сохраняя конкретный wgpu engine и существующие публичные контракты.
