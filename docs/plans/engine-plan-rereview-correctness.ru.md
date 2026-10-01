# Повторный correctness review интегрированного engine plan

Дата: 2026-10-01. Review четырёх актуальных планов; production edits и сборки
не выполнялись. Все новые сценарии ниже основаны на статическом inventory;
GPU/failure reproduction этих сценариев **не выполнялся**. Ранее выполненный
cube capture не является их проверкой. Номера строк проверены в текущем checkout.

## Закрытие предыдущих восьми замечаний

«Закрыто в плане» означает выбранный контракт, не реализованный runtime proof.

| Предыдущее замечание | Статус после правок | Проверенное основание |
|---|---|---|
| Атомарная граница всех IR families | Закрыто в плане | master:43–48 и IR plan:169–184 требуют one delivery, без dual representation |
| Неполный run target/state key | Закрыто в плане | IR plan:125–131 фиксирует extent/DPR/origin/epoch, sampler/alpha/allocation/table identity |
| Immutable только gradient stops | Закрыто в плане; новый риск submit topology ниже | master:45–46, IR plan:134–141 freeze viewport/target/effect inputs |
| Producer texels без damage/wake | Закрыто в плане | master:61–65, resource plan:34 определяют content revision и dependent invalidation/wake |
| Scene vs lowered allocation snapshot | Закрыто в плане | master:56–59, resource plan:32 намеренно latest до lowering, captured после |
| Effect regions и partial false green | Закрыто в плане | master:67–71 требует input/output/backdrop/write-clip и whole-target comparison |
| DPR и degraded capture parity | Закрыто в плане | master:69–72 и IR plan:226 требуют общий target descriptor, fractional/negative origin fixtures |
| Безграничные buffers/passes и CPU finish как retirement | Закрыто в плане | resource plan:7–11, IR plan:143–165 admission и completion owner до allocations |

Pure wgpu сохранён; нового CPU renderer нет. Gradient >8 получает full-table
execution либо explicit refusal, silent clamp запрещён (IR plan:154–160).
Breaking submission hook теперь включён в первый delivery и мигрирует реальные
Renderer/headless/embedder callers (master:112–115, review:47–52).
Ни одно из этих закрытых замечаний не надо повторять как отсутствующий контракт.

## 1. P1: submit owner ещё не означает frame transaction

**Новая блокировка конкретной реализации:** master:54 и IR plan:162–165 обещают
сохранённый last complete target при failure. Но current production отправляет
GPU работу до final frame submit: `renderer.rs:2444` отправляет clear encoder,
`layer_dispatcher.rs:434` отправляет backdrop flush, `offscreen/blur.rs:211,421`,
`offscreen/mask.rs:263`, `offscreen/blit.rs:256` отправляют свои encoders.
`frame_protocol.rs:175` clears выбранный retained target до `steps.content`,
который может вернуть ошибку; `:181` вызывает commit только после success.
Отсутствие commit не откатывает уже отправленную запись в texture.

**Сценарий:** полный прошлый кадр A retained; новый partial кадр clears damage и
успешно flushes первый backdrop; второй draw получает admission/encode error.
Последний encoder отброшен, но A уже частично переписан. Следующий recovery,
capture либо «показать last complete» может прочитать смесь A и failed frame.
Одновременно discard всех reservations ошибочно освободит capacity уже
submitted clear/effect allocations. Это статически обоснованный сценарий;
конкретный failing GPU readback пока не исполнен.

**Решение до кодирования:** выбрать transaction execution topology. Предпочтительно
preflight всех recoverable admission/validation и encode в ordered frame
command buffers без скрытых submits; sole owner submits только полностью
prepared frame. Для retained target дополнительно нужен scratch/copy-on-write
или доказательство, что после первой записи уже нет recoverable failure.
Scratch требует reservation и copy/read dependency. Если multi-submit
сохраняется ради backdrop, frame state должен явно быть PartiallySubmitted,
retirement сохраняет отправленные chunks, last-complete pixels принадлежат
отдельной неперезаписанной allocation. Одного финального hook недостаточно.

**Приёмка:** inject failure до clear, после submitted clear, после первого
backdrop chunk, на последнем effect; whole-target readback last-complete A
не меняется. У отправленных chunks reservation остаётся charged до completion;
следующий B корректно рисуется. Проверка через actual FrameProtocol, не fake
ledger, который ничего не посылает GPU.

## 2. P1: централизация submit ломает reuse доказательства blur slots

**Новая зависимость атомарной миграции:** `offscreen/mod.rs:113–125` прямо
обосновывает общий blur_uniform_buffers отдельным submit в конце render_blur.
`offscreen/blur.rs:306` пишет slot src_index через queue.write_buffer.
Перенос всех effect encoders к sole final submit без изменения slot ownership
создаёт тот же last-write hazard, который устраняется для gradient tables.

**Сценарий:** blur A 32×32 с одним radius и blur B 128×128 с другим radius
кодируются до общего submit. Slot0 первого blur и второго blur — одна allocation;
queue writes B предшествуют обоим GPU draw, blur A использует texture_size/offset B.
Если producer pass остаётся unsent в общем encoder, а прежний render_blur всё
ещё сам submits, он может ещё и прочитать прошлые producer texels вместо
текущего кадра. Это статический сценарий, не исполненная регрессия.

**Решение:** inventory submit sites должен сопровождаться inventory reuse
assumptions каждого ресурса, не только заменой queue.submit на owner method.
Every blur invocation/iteration/direction получает уникальный immutable per-use
binding до submission, либо единый pool cursor с невозможностью reset между
chunks. Producer→copy→blur→composite edges остаются в одном ordered plan;
queue.write_buffer нельзя трактовать как command, расположенный между draws.
Такая миграция входит в первый owner/immutable-parameter delivery.

**Приёмка:** два различных blurs в одном submit и producer цвета текущего frame;
сравнить каждый output с отдельным fresh reference GPU draw. Revert per-use
blur bindings при сохранённом submit centralization обязан ломать первый blur.
Также два sequential frames с delayed completion не alias allocations.

## 3. P1: Result final hook не закрывает уже swallowed failure

Current `layer_dispatcher.rs:404–406` при ошибке painter.render только логирует
«Backdrop flush failed», затем копирует surface region и submits encoder в
`:434`. Typed frame ResourceError / first-error-authoritative в resource plan
не помогает, если error никогда не достигает frame owner. Новый submission hook
может добросовестно отправить такой chunk как успешный.

**Сценарий:** replay backdrop input возвращает ошибку при подготовке ресурса;
copy/blur продолжаются на incomplete source, render_scene может прийти к present.
Next valid test, вызывающий только новый allocator reject seam, не увидит этот
путь. Это реально прочитанный swallowed-error branch; failure injection/GPU
reproduction ещё не выполнен.

**Решение:** до implementation inventory всех log-and-continue, Option→skip и
error→fallback веток в frame lowering/effects. Для каждого выбрать deliberate
recoverable approximation либо authoritative frame failure; current backdrop
flush должен возвращать failure к transaction owner до copy/submit. Узкие helper
return signatures мигрируют вместе с callers. «Неподдерживаемое — отказ» нельзя
оставлять только doc assertion в registry.

**Приёмка:** inject actual painter replay error именно в backdrop flush, проверить
first error, отсутствие successful present/copy from poisoned input, сохранность
last-complete target и следующий валидный backdrop frame. Дополнительный cleanup
fault не подменяет исходную причину. Test не должен обходить dispatcher напрямую.

## Итоговое решение перед первой реализацией

Предыдущие восемь design gaps закрыты в плане. Следующий обязательный design
артефакт — inventory всех production submit sites и их ресурсных reuse/error
assumptions плюс выбранная transactional retained-target topology. Rust signature
нового submission hook определяется **после** этого inventory: final-only hook
не обеспечивает уже заявленные guarantees. Эти три находки связаны, но их proof
различается: rollback pixels, immutable parameters, propagation first failure.
Новых публичных backend/Scene hooks и другого rasterizer для решения не нужно.
