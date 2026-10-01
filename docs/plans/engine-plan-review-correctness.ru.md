# Adversarial review: correctness и frame ownership плана engine

Дата: 2026-10-01. Только review; production edits и сборки не выполнялись.
Ссылки ниже — проверенные номера строк текущего checkout. P1 означает блокировку
соответствующей миграции до уточнения решения, а не заявление о уже реализованном
дефекте новой архитектуры. Где production fail ещё не выполнен, это отмечено.

## 1. P1: граница первого IR PR противоречит собственному плану

Master `engine-foundation-implementation.ru.md:47` требует один ordered stream
для всех families и удаление старого category path. Детальный
`engine-frame-ir-migration.ru.md:112–126` допускает gradient fix, затем переход
families, затем отделение immutable recorder отдельными поставками. Это оставляет
две competing истины порядка и movable mutable segment между PR. Текущий
`command_ir.rs:642` документирует fixed category replay, gradient исключён из
Phase. Сценарий: linear → radial → linear либо glyph → gradient → rect пересекаются;
fix только gradient→solid может пройти свой test и сохранить reorder внутри family.

**Решение:** выбрать один acceptance boundary: все producers пишут typed ordered
runs, все offscreen consumers читают их, old Phase/category scheduling удалён,
sealed ownership enforced в том же PR. Можно иметь несколько локальных commits,
но green intermediate commits не объявлять поставленным foundation. Альтернатива
— честно выделить узкий gradient correctness PR отдельно от foundation; тогда
master не должен называть его завершённой immutable IR миграцией.
**Проверка:** public table всех ordered пар kinds плюс linear→radial→linear;
revert только ordered-run dispatch должен падать независимо от stop-table fix.

## 2. P1: ключ совместимости run не задаёт target/frame state

Детальный план `engine-frame-ir-migration.ru.md:59–72` называет pipeline/blend,
scissor/clip/resource keys, но не определяет, что такое равенство target-relative
state. `replay/flush.rs:27` уже объясняет rebase full-viewport scissor в tile;
`instancing.rs:112–115` хранит clip-local transform. Сценарий: два одинаковых
scissor numbers относятся к разным offscreen origins или разным viewport epochs;
merge переносит clip/gradient coordinates в другой target. Это гипотеза ошибки
будущего merge, не утверждение о новом коде.

**Решение:** run живёт только внутри одного sealed target context. Target identity,
physical extent, device-to-target origin, viewport epoch и immutable clip reference
фиксировать на segment/prepared-pass boundary; merge физически не пересекает эту
границу. Per-run key дополнить effective alpha pipeline, effective sampler,
allocation identity и gradient-kind/table identity там, где они не являются
segment invariant. Не сравнивать float matrices приблизительно ради batching.
**Проверка:** одинаковый local clip на двух rebased tiles разного origin,
два gradients разных kinds и stop tables, straight/premul texture с одинаковым
TextureId number из разных generations. Сравнивать внутренние и внешние samples.

## 3. P1: исправление stop buffer не закрывает общий mutable-upload hazard

Master `engine-foundation-implementation.ru.md:41–44` ограничивает lifetime stops,
но не задаёт admission между flushes. `replay/mod.rs:215–220` пишет viewport
uniform через queue.write_buffer offset0, `painter/mod.rs:527` делает это при
resize. Публичный embedder может encode первый render_to_view, вызвать resize,
encode второй и submit оба вместе. Первый shader увидит последний viewport.
Это конкретный API-сценарий; pixel test на нём ещё не запускался.

**Решение:** выбрать и enforce: resize запрещён внутри recording/encoded-not-submitted
frame либо viewport bindings frozen per prepared target. Первый вариант дешевле,
но public begin/finish должны действительно выразить state, а не быть инструкцией
в doc comment. Провести inventory всех queue.write_buffer до общей submission;
различать уникальный slot на pass и shared mutable slot. Не объявлять immutable
IR доказательством immutable GPU parameters.
**Проверка:** два public flush с различными viewport sizes до одного submit;
первый sample должен соответствовать своему viewport либо API обязан вернуть
typed refusal до изменения первого frame. Повторный valid frame после refusal.

## 4. P1: volatile texels не связаны с damage/wake

Resource plan `engine-resource-contract-migration.ru.md:26` отдельно разрешает
latest texels в одной allocation; master `engine-foundation-implementation.ru.md:64`
повторяет эту политику. Но ни один план не задаёт production owner, который
превращает producer write в redraw/damage. `frame_protocol.rs:105–106` принимает
решение по накопленному damage. Сценарий: retained frame содержит external
texture; producer обновляет её без смены lease/Scene, следующий frame NoDamage,
экран продолжает показывать старые пиксели. Allocation generation не меняется,
поэтому новые identity/cache keys сами это не чинят.

**Решение:** в resource contract сейчас определить content revision и owner
уведомления: producer commit должен invalidate все зависимые output regions и
wake соответствующий realm, либо volatile resource принудительно full-repaint
при каждом запрошенном frame плюс explicit wake producer. Не ждать SDK PR5 для
самого правила; implementation managed transport может последовать, но first
consumer обязан демонстрировать имеющийся production path.
**Проверка:** same allocation A red→green, unchanged Scene, quiescent host;
commit producer приводит к новому present зелёного результата. Проверить также
независимые окна/realms: wake одного не повреждает другой.

## 5. P1: Scene identity и lowered lease freeze имеют невыбранную временную границу

Resource plan `engine-resource-contract-migration.ru.md:24` захватывает lease
«во время record»; master `engine-foundation-implementation.ru.md:63` говорит
«ранее sealed frame». Но Scene заморожен выше engine и хранит TextureId;
`command_ir.rs:610–619` описывает позднюю resolution, `replay/flush.rs:936`
делает lookup сейчас. Сценарий: Scene A сформирован и передан raster mailbox;
до lowering registry ID rebound на B. Record-time freeze честно заморозит B,
хотя caller считал Scene snapshot A. Эта миграция может пройти register→record→
unregister test и не проверить настоящую scheduling границу.

**Решение:** письменно выбрать: SourceScene является ссылкой на latest allocation
до начала lowering либо snapshot захватывает allocation identity при Scene
handoff. Не обещать оба. Для first engine-local change корректно выбрать первый
вариант, называть snapshot только sealed lowered frame и добавить test Scene
created→rebind→lower. Если нужен SourceScene snapshot, это cross-crate ADR и
generation-bearing resource reference, не тайный wgpu handle во flui-layer.
**Проверка:** отдельные sequence до Scene build, между build и lowering,
между seal и encode, между encode и submit; ожидаемая allocation явно задана
для каждой строки. Content write same allocation проверяется отдельно.

## 6. P1: effect region недостаточен для partial-damage soundness

Master `engine-foundation-implementation.ru.md:94–100` требует source/destination
region и partial-vs-full, но не различает input dependency, expanded output,
backdrop read region и write clip. Сценарий: изменился маленький красный rect,
blur распространяет изменение вне первоначального damage; sample только внутри
damage остаётся правильным, старый halo снаружи сохраняется. BackdropFilter
дополнительно зависит от ранее нарисованных siblings, а не только своих children.
Текущий `frame_protocol.rs:165–181` выбирает partial target и commits retained
content; исправление одного clip chain не делает такой план безопасным.

**Решение:** до clip/effects implementation определить canonical device-space
четыре области и conservative dependency propagation: input→output expansion,
backdrop dependency→output invalidation, destination-replacing blend write region,
clip ограничивает output, но не обязательно source sampling. Unsupported/nonlocal
effect заставляет full frame, не недоказанное partial. Layer bounds и image
source rect не interchangeable с output region.
**Проверка:** сначала полный blur/backdrop кадр, изменить child либо задний sibling,
нарисовать partial, сравнить весь target с fresh full frame, включая halo вне
исходного damage и unchanged distant pixels. Проверять также rotated clips,
DPR и negative offscreen origin. Revert damage expansion обязан ломать test.

## 7. P1: capture parity можно ложно доказать одним device-space примером

Master `engine-foundation-implementation.ru.md:97–99` говорит window/headless
capture equivalence. `headless.rs:182–207` принимает physical size; пример
`embedded_gpu_scene.rs:327–346` строит UI прямо из physical config dimensions.
Успешный capture куба не проходит logical→device geometry pipeline, host DPR,
layer transform или divergence ShaderMask/BackdropFilter/Follower, перечисленные
в `headless.rs:20–22`. Это ограничение текущего доказательства, не провал cube demo.

**Решение:** equivalence acceptance должен использовать один SourceScene и один
lowering target descriptor {logical extent, DPR, physical extent, target origin}.
Renderer/capture не передают независимо придуманные масштабы. Не добавлять второй
CPU renderer: оба oracle outputs wgpu, эталон — fresh full draw и аналитические
sample expectations на deliberately different координатах.
**Проверка:** DPR1/1.5/2, rect с дробным logical origin, nested translation/rotation,
нецентральный sample, follower после leader offset, shader mask и backdrop реально
меняют цвет. Одинаковые Scene + descriptor обязаны совпадать в допустимой GPU
AA tolerance; degraded headless path не служит reference.

## 8. P2: correctness-first allocation не имеет bounded worst-case workload

Master `engine-foundation-implementation.ru.md:41–44` и детальный
`engine-frame-ir-migration.ru.md:78–96` создают отдельный STORAGE allocation на
segment и откладывают reuse. Сценарий: 10000 маленьких alternating gradients/
solids либо save-layer barriers: тысячи buffers/bindgroups/passes за кадр,
хотя stop bytes малы. Adjacent-only fusion необходима для порядка, но сама не
ограничивает work. Resource plan `engine-resource-contract-migration.ru.md:34–38`
обещает CPU/pass limits позже; allocation-first IR может выйти до них.

**Решение:** first IR PR требует quantitative run/segment/stop-byte/prepared-pass
admission до создания buffers, не универсальную оптимизацию. Для обычного UI
зафиксировать comparison workload old vs ordered IR (draw calls, pass count,
CPU preparation time, transient bytes). Если immutable-per-segment buffers
слишком дороги, единая frozen frame stop arena с validated slices — допустимый
вариант в том же correctness boundary, но только без shared mutable overwrite.
**Проверка:** contiguous10000 compatible gradients coalesce, alternating kinds
сохраняют порядок; превышение установленного budget возвращает typed error без
partial present и следующий small frame работает. Порог задаёт FLUI policy,
а не test, который только повторяет private constant.

## Решения, которые parent должен принять до production edits

1. Один законченный ordered/sealed IR PR либо явно отдельный narrow gradient
   fix; не оставлять двусмысленную «foundation done» границу.
2. Freeze boundary: SourceScene latest до lowering, lowered frame captured leases;
   либо cross-crate snapshot identity уже сейчас.
3. Resize/viewport policy между flushes и producer content invalidation owner.
4. Canonical effect dependency regions + whole-target partial equality matrix.
5. Admission caps first correctness PR; benchmark/reuse являются следующей
   измеряемой задачей, не поводом сохранить неправильный order.

Принятие этих решений не вводит альтернативный backend, публичные arena indices
или второй Scene API. Оно делает план исполнимым и защищает от green tests,
которые проверяют только специально выбранный subset.
