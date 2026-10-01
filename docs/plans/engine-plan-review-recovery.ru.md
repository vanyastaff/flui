# Критическая проверка плана: admission, recovery и authority результата

Проверены master `engine-foundation-implementation.ru.md`, IR `engine-frame-ir-migration.ru.md` и ресурсный `engine-resource-contract-migration.ru.md`. Ниже восемь блокирующих неопределённостей **плана**, с воспроизводимыми сценариями и исправлениями. Это не утверждение о выполненной миграции. Production-код не изменялся, сборки не запускались; реальный driver OOM, потеря устройства и native timing не проверены.

## 1. Budget после IR позволяет исчерпать память первым PR

Master §1 и IR «Stop tables и GPU lifetime» создают отдельный STORAGE buffer/bindgroup на segment; единый admission отложен в resource migration. Миллион двух-stop segments меньше per-binding limit, но исчерпывает host/GPU память до PR2. Проверка device storage limit не ограничивает число таблиц. `Vec` recorder, seal→Box, временные conversion arrays и staging могут одновременно удерживать несколько копий; только GPU bytes их не покрывают.

**Исправление:** минимальный shared resource ledger и CPU work/bytes admission входят в PR1 до `Vec` роста, table preparation и create_buffer_init. Checked reservation включает peak recorder+sealed+temporary copies, число runs/stops/bindgroups, staging и GPU payload. `try_reserve` позволяет обработать поддержанные allocator failures, но не обещает containment любого Rust OOM/process abort. Отказ последнего segment оставляет previous completed frame; следующий маленький frame работает. Полный cache ledger можно расширить позже.

## 2. finish_frame не означает GPU completion

Master §1 говорит «до submission completion»; IR разрешает resources удерживать закодированным wgpu commands; resource plan требует accounting до retirement. Эти обещания различны. CPU drops после submit освобождают ledger, пока GPU ещё использует allocation; при медленном GPU очередь из ста кадров проходит каждый новый бюджет и сохраняет сто старых allocation. Wgpu handle retention гарантирует safety, но не FLUI cap.

**Исправление:** один submission owner забирает frame reservations и список retired allocations. Ledger освобождается после completion конкретного submission либо окончательной teardown потерянного device generation. Ввести ограничение submitted frames/bytes и typed NotReady с durable wake obligation; callback не пишет presentation state и не вызывает произвольный user Drop. Отдельный тест задерживает completion, проверяет admission отказ, затем callback и next frame. Public embedder, способный сам submit arbitrary encoders, обязан участвовать в этом protocol; простой void finish_frame не доказывает lifecycle.

## 3. Strong lease не защищает raw allocation от destroy

Resource plan правильно различает freeze allocation и texels, но master acceptance «unregister/update → original allocation» неполон для raw import. Caller сохраняет clone Texture, после record вызывает `destroy()`; Arc lease удерживает object, но allocation уже destroyed. Другой Device или внешняя Queue не становятся доверенными из-за owner-token поля. Импорт происходит после внешней allocation и не способен предотвращать её memory cost.

**Исправление:** гарантии различать типами/entry points: managed allocation без выдачи destroy authority и restricted producer usage против explicit trusted raw import. Managed factory связывает owner/device generation до allocation. Raw import документирует caller obligations и проходит wgpu validation; внешние clones/allocations не входят в обещание полного cap. При destroy/invalid device frame получает failure, а не «lease всё гарантирует». Тесты external raw misuse — отдельная failure family; unknown provenance не silent success. Конкретный механизм wgpu ошибки требует проверки выбранной версии API, не предполагать introspection.

## 4. Queue writes меняют pixels без смены generation

IR «Queue lifetime» и master §2 допускают volatile contents, но нельзя обещать «original frame» без уточнения. Record red A; producer `queue.write_texture` blue A; submit draw A — результат blue. Record два uses одной allocation, между ними CPU writes red/blue перед одним submit — оба могут увидеть последнее содержимое. Другой producer queue/encoder submission дополнительно меняет порядок независимо от lease.

**Исправление:** allocation generation и content revision разные понятия. Explicit policy `LatestAtOrderedSubmission` разрешает изменяемые pixels, но имеет единственного submit owner и определённый порядок producer passes. Snapshot требует новых allocations или GPU copies с dependency edges и бюджетом; content generation token сам pixels не сохраняет. Acceptance говорит «allocation A», отдельно тестирует volatile blue и snapshot red. Managed producer contract PR5 должен зафиксировать это до выдачи публичной Queue capability.

## 5. Sampling/alpha/source space могут снова противоречить друг другу

Master §2 «сохраняет выбранный sampler» не выбирает authority между resource descriptor и public per-draw FilterQuality. Текущий `batches/images.rs::draw_texture` игнорирует `_filter_quality`; registry update переключает nearest на linear; replay bindgroup всегда default linear. Исправление только registry оставляет consumer неправильным. Premul sample `[0.5,0,0,0.5]` при opacity0.5 через premul pipeline с tint `[1,1,1,0.5]` сохраняет RGB0.5 вместо0.25. Color PR4 затем может интерпретировать encoded premul texels как linear и производить другую ошибку.

**Исправление:** per-draw sampling authoritative, resource-default режим выбирается явно; mip/high quality заявлены только если поддержаны. Descriptor фиксирует source encoding и alpha; working pipeline — отдельный тип. Premul opacity масштабирует все четыре компоненты. Неподдержанные color/alpha combinations rejected до recording/preparation; no clip до deliberate output mapping. Descriptor generation включает metadata, чтобы cache не переиспользовал старую трактовку. Readback матрица проверяет nearest/update/override, premul opacity, source conversion, zero alpha и >1; shader clipping/cutoff нельзя спрятать CPU storage тестом.

## 6. Logged uncaptured error не может подтверждать успешный frame

`renderer.rs::install_device_diagnostics` сейчас `on_uncaptured_error` только tracing; device loss отдельно ставит flag. Master PR5 различает accepted/committed/displayed, но не задаёт attribution и authority. Ошибка bindgroup/OOM может прийти после CPU возврата и present_submitted; host уже сообщил frame committed как correct, хотя draw не выполнен. Shared producer использует тот же device, и его error callback не должен быть заменён последним owner.

**Исправление:** один device diagnostics owner с generation-aware fault mailbox и ограниченным retention. Scoped validation/OOM checks для owned preparation, bounded async polling на IO/host edge, synchronous frame path. Submit accepted, successful GPU completion и actual displayed — разные acknowledgments; completion callback сам не доказывает scanout или pixel correctness. Unattributed uncaptured error маркирует generation fault/uncertain outcomes, запускает deliberate recovery, а не назначается произвольному последнему frame. Нужны tests delayed error, producer+UI error competition и next recovered frame; никакое error scope не обещает поймать driver crash.

## 7. RAII cleanup без first-error policy может уничтожить progress

Master PR5 требует RAII; resource plan «первый error authoritative». Сценарий: admission отказ при mask preparation, затем rollback Drop reservation/producer callback паникует; double panic during unwinding aborts process, а `finish_frame` не выполнен. Или submit успешен, ошибается retirement callback, который освобождает ledger дважды. Нынешний public painter begin/finish — manual void pairing, а не доказанная frame transaction.

**Исправление:** inventory owned values каждого boundary; internal reservation/lease Drops infallible и idempotent без user callbacks. User-defined destructors/callbacks исполнять только в контролируемых boundaries с принятой panic policy, сохранять первый Result/panic, последующие faults диагностировать отдельно. Нельзя обещать catch любого Drop, panic=abort или OOM. Prepared/Submitted/Discarded ownership state исключает повторный release; rollback до submit и retirement после submit разные операции. Failure matrix включает каждую ошибку отдельно, две в обеих последовательностях, затем valid frame, включая clip stack и retained target state.

## 8. Cancellation token не ограничивает CPU future или GPU job

Master PR6 делает cancellation обязательной, но срок только сетевого request не покрывает worker. Task future выполняет большой decode/shader preparation без await; cancel/replacement происходит, future не уступает executor и IO service не завершается вовремя. GPU command после submit также нельзя «отменить Drop TaskHandle». `ai_streaming` уже ограничивает response bytes/events и yields между UI events; это ограниченный пример, а не универсальная task liveness guarantee.

**Исправление:** отдельно deadlines, byte/work budgets, cancellation checkpoints и max concurrent/queued jobs. Cooperative async cancellation — гарантия только для yielding bounded steps. Blocking workers получают bounded admission и отдельный shutdown policy; процесс изоляции нужен лишь там, где требуется hard interruption untrusted native work. Results проверяют owner/generation после завершения, reservations остаются charged до фактической retirement, а не до UI Stop. Tests distinguish canceled UI publication, observed transport disconnect, worker completion и GPU retirement. Timeout не использовать как доказательство освобождения ресурса.

## Что интегрировать перед началом implementation

В master PR1 перенести минимальные admission/reservation и submit retirement ownership. В PR2 определить raw/managed гарантию и sampling precedence. В PR5 до public capability зафиксировать sole submit/diagnostics owner и volatile-content contract. PR6 ограничить cancellation claims. В каждой acceptance таблице должны быть reject→next valid operation и competing failures, а не только успешный pixel. Частично выполненные callbacks/enum/types не считать production contract до реально достижимого consumer и negative proof.
