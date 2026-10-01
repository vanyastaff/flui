# Adversarial API review: engine migration plan

Дата: 1 октября 2026. Проверены master plan, frame-IR plan, resource plan и
color proposal. Ниже восемь решений, без которых реализация либо меняет семантику
случайно, либо не имеет проверяемого окончания. Production и сборки не менялись.
Pure wgpu сохранён; alternate renderer/oracle не предлагается.

## 1. Sealed IR в первом PR или после correctness: планы противоречат

[Master §1](engine-foundation-implementation.ru.md#1-заморозить-ordered-gpu-command-ir)
требует sealed ordered IR для всех families; [frame plan, минимальная последовательность](engine-frame-ir-migration.ru.md#минимальная-последовательность)
ставит Recorder/Segment separation после исправления order и допускает сначала
сосуществующие representations. Сценарий: gradient→tessellation→image под filter
правилен в новом run stream, но старый flush category array повторно рисует Tess.

**Решение:** первый atomic PR меняет stop ownership и actual replay ordering
вместе. Если full sealed refactor слишком велик, вводится private ordered command
index, единственная consumed representation; legacy arrays только backing storage,
никакого второго replay. Следующий PR меняет mutability без нового поведения.
В acceptance явно mixed family + nested segment и отсутствие duplicate draw.
Не требовать 18 файлов в одном PR ради абстрактной чистоты.

## 2. Gradient range без поддержанного stop count оставляет старое ограничение

Frame plan описывает Box<[GradientStop]> и storage limits, но не решает semantics
нынешнего восьми-stop gradient ([capability audit](../research/engine-capability-audit.ru.md#карта-возможностей)).
Сценарий девять чередующихся opaque stops: tail silently отсутствует даже при
правильных bindings и painter order; structural immutable-IR test этого не ловит.

**Решение:** первый PR сохраняет explicit documented limit с typed rejection,
либо actual shader dynamic count/index loop поддерживает весь admitted диапазон.
No truncation. Test 0/1/8/9 stops, repeated offsets/hard stop, unsorted/NaN offsets
и next valid frame. Ramp library color не исправляет shader cardinality автоматически.

## 3. Allocation lease не гарантирует survival при caller Texture::destroy

[Resource lifetime](engine-resource-contract-migration.ru.md#следующее-обязательное-изменение-allocation-identity-и-leases)
предписывает не destroy live allocation, но raw import оставляет producer raw clone.
Сценарий record A → producer.destroy(A) → submit: lease всё ещё strong, GPU resource
invalid. Same-device owner token также не доказывает provenance.

**Решение:** managed resource factory гарантирует доступ лишь к scoped producer
capability, не передаёт destroy authority; trusted raw-import contract явно исключает
external destroy/mutation safety. API name и errors отражают доверие. Tests separately
pin strong-handle lifetime и validation-failure recovery; не заявлять raw-import
memory cap или content immutability. Production example должен выбрать один режим.

## 4. CPU ledger completion зависит от host; finish_frame не completion

Resource plan требует submitted retirement, master говорит finish/submission cleanup;
[frame plan](engine-frame-ir-migration.ru.md#stop-tables-и-gpu-lifetime) различает flush
и finish. Сценарий embedder submit несколько encoders, finish_frame освобождает
reservation, GPU ещё выполняет дорогой producer; следующий frame admission превышает
обещанный payload cap. Противоположный отказ: host не poll, completion callbacks не
наблюдаются, budget навсегда exhausted.

**Решение:** ledger lifecycle recorded→submitted(serial)→retired(completion), discard
только unsubmitted. Host pump обязан доставлять completion; retained resources в
device teardown освобождаются отдельным доказанным событием. Бюджет может typed
Throttle/Wait/Reject, нельзя silently считать finish completion. Acceptance два
in-flight frames, exhausted admission, delayed completion, failure+recovery, window
close с pending callbacks; frame API outcome не новый необслуживаемый pub hook.

## 5. Новый color intent и линейный working target нужны разными PR

[Color proposal](../research/color-foundation-adoption.ru.md#минимальная-производственная-миграция)
верно требует менять filters/blends/text/images вместе, master §4 не выделяет
reviewable boundary между сохранением intent и линейной композицией. Сценарий
vertex выдаёт linear RGB, старый texture shader/UNORM target считает его encoded;
полупрозрачный белый на чёрном темнеет и color matrix меняет meaning.

**Решение:** PR-A только owned tagged source+validated serde/hash+explicit named
legacy SDR conversion во всех production consumers. PR-B единый working transition
source→linear premul→effects→encode, без feature mix внутри кадра; effect matrix
domain и advanced blend formula space закреплены отдельно. Existing Color proposal
уже правильно запрещает NaN/Infinity/-0 drift; acceptance добавляет deserialize
bypass, overflow conversion и const byte convenience. RGBA16F gamut headroom не
объявлять HDR presentation. No unused HDR enum/helper без production caller.

## 6. Glamx singularity check не conditioning policy и не projective clip

[glamx MatExt 0.3.1](https://docs.rs/glamx/0.3.1/glamx/trait.MatExt.html)
даёт try_inverse, SVD; не обещает tolerance robust UI transform. Existing
[Transform::inverse](../../crates/flui-foundation/src/geometry/transform.rs) уже
возвращает Option, general branch зовёт matrix.try_inverse.
Сценарий diag(1e-30,1) конечен и invertible, но roundtrip после GPU f32 narrowing
теряет геометрию; perspective quad пересекает w=0 и corners AABB не ограничивает его.

**Решение:** пока нет actual SVD/tolerance consumer — не добавлять glamx. Выбрать
supported affine/projective policy, finite/conditioning threshold и crossing-w
clip/rejection на owned f64 boundary; logical/device units не glam aliases.
Acceptance singular/near-singular/huge translation/reflection/w-zero independent
fixtures. Numeric guard не должен изменять корректный tiny local geometry из-за
одного универсального epsilon. DSvd2 применим к affine linear part, не full projection.

## 7. Supersession нельзя заменить правкой принятого ADR

[Frame plan](engine-frame-ir-migration.ru.md#adr-draft-для-обсуждения) предлагает
amend ADR-0006, resource plan «обновляющий» его cross-crate contract.
AGENTS.md ADR Policy требует новый `Supersedes` и old `Superseded-by`, когда код
меняет принятый decision. Сценарий logical TextureId становится generational
owner-scoped, Scene contract меняется, старый ADR молча переписан без migration trace.

**Решение:** private engine IR clarification может Mapping decision без нового
ADR, если accepted invariant сохранён. Cross-crate lifetime/identity или working
color decision получает новый Proposed superseding ADR с exact sections и migrated
consumer list. Proposed ADR-0098 можно исправить до acceptance; не называть его
accepted только из-за implementation. Старые accepted sections явно superseded,
не переписаны задним числом. Package SDK API только с actual production caller.

## 8. Бюджеты и performance обещаны без стартовых чисел

Все три плана говорят measured budgets, но не фиксируют workloads, p50/p99 или
границу regression. Сценарий immutable per-segment storage correct, однако тысяча
чередующихся gradients создаёт тысячу buffers/bindgroups, frame stalls; green pixels
и отсутствие second backend этого не замечают. Resource ledger измеряет payload,
не точный driver VRAM, что resource plan правильно признаёт.

**Решение:** до implementation capture baseline на одном adapter/build/profile для
solid, alternating gradients, nested clips/effects, image cache churn и text atlas
pressure. Отдельно CPU record/prepare/encode p50/p99, GPU timestamps если доступны,
allocation/upload bytes, live+retired peak payload, pass/bindgroup counts.
Root выбирает допустимые regression bounds после baseline; не выдумывать числа.
PR1 correctness может временно принять измеренный рост allocations с конкретной
follow-up storage arena, но не бессрочную декларацию «оптимизируем потом».
Capture/automation reuse existing desktop-mcp; async completion measurement не
подменять screenshot arrival. Compare identical scenes, warm/cold и sample counts,
избегая benchmark marketing и exact cross-machine p99 promises.

## Требуемые решения root перед production

Согласовать единственный atomic IR ordering representation; gradient count policy;
trusted import vs managed ownership; submission retirement/host pump; два color
PR boundaries; projective availability; ADR supersession mapping; measurement
workloads и критерии regression. Эти решения делают существующий план выполнимым
сейчас и не требуют нового универсального SDK/agent framework.
