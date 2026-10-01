# Повторная проверка recovery: оставшиеся решения

Дата 2026-10-01. Прочитаны обновлённые master, consolidated review, frame IR и resource plan. Production не менялся, сборки и failure experiments не выполнялись. Ниже реальные пропуски решения, а не повтор уже интегрированных замечаний. Пять пунктов блокируют соответствующий implementation boundary; описанные будущие failures — гипотезы, подкреплённые текущими submit sites и API, но не новые выполненные regressions.

## Закрытие прежних восьми замечаний

| Прежнее замечание | Статус после интеграции |
|---|---|
| Admission отложен за immutable tables | Закрыто на уровне решения: master:49–55, resource:7 требуют GPU/CPU peak/count admission в первом delivery. Whole-engine cap ещё не заявлять до полного ledger |
| finish_frame принят за completion | Закрыто принципиально: IR:104–111 добавляет engine-owned handoff; конкретные multi-submit и close states остаются ниже |
| Strong lease считается защитой raw destroy/provenance | Закрыто для texture: resource:36 различает managed и trusted import; device capability требует уточнения ниже |
| Allocation lease считается pixel snapshot | Закрыто: resource:32–34 выбирает latest до lowering, отдельный copied snapshot, content revision/damage/wake |
| Sampling/alpha/working encoding неопределённы | Закрыто: resource:21–23 и master:64–75 задают authority и coherent working transition |
| Logged error подтверждает frame | Закрыто принципиально: resource:50 вводит generation faults и различает acknowledgments; error-scope ownership ещё ниже |
| RAII объявлено универсальным recovery | Закрыто: resource:52 ограничивает infallible internal Drops и первый failure; partial submission требует отдельного state |
| Cancellation объявлено hard CPU/GPU stop | Закрыто: resource:52 и master:82–84 явно cooperative, bounded steps и retirement отдельно |

## 1. Handoff финального frame не покрывает уже submitted части

IR:105–108 принимает command buffers/reservations и сам submit; resource:9 имеет Recorded→Submitted→Retired. Однако production frame уже отправляет clear (`renderer.rs:2444`), dispatcher flush (`layer_dispatcher.rs:434`), mask (`offscreen/mask.rs:263`), blur (`offscreen/blur.rs:211,421`) и blit (`offscreen/blit.rs:256`) до final submit (`renderer.rs:2519`). `offscreen/mod.rs:123` даже документирует once-at-end, что не совпадает с этим inventory.

Сценарий: ранний clear/blur submitted, позже allocation mask отклонена. Нельзя discard всю frame reservation как unsubmitted. Если ранний pass изменил retained complete target, обещанный previous completed frame уже разрушен. Единственный final hook не устраняет этот defect.

**Выбрать до кода:** либо prepare/reserve всё до первого submit и encode весь frame с одним ownership handoff; либо handoff каждого subsubmission, shared allocation refcounts/last-use serial и FrameState с набором submitted serials. Во втором случае поздний отказ retire отправленное и discard только остаток. Target staging/double buffering либо эквивалентная доказанная commit discipline сохраняет previous complete target; «не present» недостаточно для mutated retained target. Проверка fault после первого blur submit, ledger peak, unchanged previous target, затем valid frame. Raw standalone test submits не должны маскировать production submits.

## 2. Native close может остановить pump раньше retirement

Resource:9 требует host pump и «доказанной teardown», IR:150–152 — completion owner, но отсутствует bounded closing/lost transition. Wgpu30 callback требует submit/poll; если закрытие последнего окна останавливает loop, pending reservation больше не получает completion. Callback захватывает owner Arc, который удерживает Device: «drop window» не доказывает teardown. Poll Wait в UI path может зависнуть/заблокировать shutdown при stalled GPU.

**Решение:** generation-owned retirement service переживает окна и имеет Closing/Draining/Lost/Retired состояния. Completion callback только sends `(generation,serial)` в mailbox, без захвата сильного device/realm owner и user Drop. Native pump продолжает bounded nonblocking polls, WebGPU использует browser event progress; PollError/loss переводят состояние явно. Deadline прекращает ожидание UI, но не объявляет VRAM освобождённой; отдельно снять owned handles и завершить generation teardown. Late callback старого поколения idempotent. Проверки close с pending work, loss перед callback, timeout и поздний callback; не считать таймер доказательством GPU completion.

## 3. ErrorScopeGuard не переносится вместе с async task

Resource:50 говорит «error scopes там, где поддерживает wgpu» и host edge, но отсутствует capture-boundary protocol. В wgpu30 scopes thread-local при std, guard !Send/!Sync; Drop без явного pop теряет captured errors. Сценарий preparation returns Err, guard dropped, OOM diagnostic пропадает; либо producer IO thread делает device operation вне render-thread scope и frame incorrectly ждёт clean scope. Scoped validation не гарантирует attribution async other-thread operations.

**Решение:** каждый owned device-operation batch на своём thread пушит фильтры, выполняет операции и явно pop guards в обратном порядке даже на early error. Pop немедленно закрывает scope; future доставляется bounded edge resolver с generation/operation id. Переданные futures не содержат guard. Validation/OOM/Internal filter selection и unresolved outcome policy зафиксировать; scope failure вторичный к уже известному primary error. Uncaptured other-thread fault остаётся generation-wide uncertain. Тест early rejection+captured validation competition, guard pop при unwind и producer thread fault; mapping checks не await внутри paint.

## 4. Первая минимальная quota и общий cap остаются смешаны

Master:49–55 вводит submitted bytes/frames, resource:42 описывает единый live+pool+cache ledger, resource:44 откладывает cache/atlas/buffers. Сценарий immutable tables помещаются в cap, но действующий image upload, pooled blur и glyph atlas потребляют ещё сотни MiB вне ledger. Count limits не ограничивают размер отдельного effect texture. Это допустимое incremental scope, если contract первого delivery честно называется quota новых ресурсов, а не whole-engine cap.

**Выбрать:** либо первый общий cap инвентаризирует все active allocation sites/bytes до заявления cap; либо ограниченное `PreparedIrBudget` явно считает только новые tables/uniforms/instances и свои CPU temporaries, а legacy allocations имеют отдельные quotas/telemetry с перечисленными exclusion. Следующий whole-ledger delivery устраняет exclusions. При переносе allocation между prepared/pooled/cache/submitted categories charge не сбрасывается и не дублируется. Проверка combined workload table+blur+atlas+image, refusal и next small frame; метрика driver VRAM отдельно от payload.

## 5. Scoped raw Device/Queue выдаёт больше authority, чем managed promise

Master:195 выдаёт scoped device/queue/output capability; resource:36 обещает managed без destroy authority, IR:109 исключает trusted raw queue из cap. Но `&wgpu::Device` можно clone и `Device::destroy()` принимает `&self`; `&Queue` позволяет clone и concurrent submits после окончания scope. Lifetime wrapper над reference не ограничивает эти методы. Сценарий producer клонирует Device/Queue, destroy уничтожает весь UI generation или submit обходит sole owner; это шире уже закрытого Texture::destroy.

**Выбрать сейчас:** raw wgpu Device/Queue API — trusted embedder mode, contract явно исключает managed authority/order/cap guarantee. Managed producer получает FLUI operation capability, действительно не раскрывающую raw Device/Queue и не возвращающую clonable handles с запрещённой authority; async queued requests валидируют generation. Не делать обёртку над Deref<Device> «безопасной». Если managed factory внутри доверенного embedder, сформулировать «engine-owned accounting under trusted producer obligations», а не adversarial isolation. Acceptance включает actual compile/public API проверки утечки clone authority и trusted violation typed generation failure где возможно.

## Проверенные первичные API

Cargo.lock фиксирует wgpu30.0.1. Локальные опубликованные исходники `wgpu-30.0.1/src/api/queue.rs:305–324`: `on_submitted_work_done` относится к предыдущему submit, требует submit/instance.poll_all/device.poll; native polling call ждёт возврата callback, поэтому он должен быть коротким. Queue.submit возвращает SubmissionIndex, не Result и не proof successful displayed frame. Source `device.rs:89–102`: poll возвращает Result<PollStatus,PollError>, Wait блокирует; WebGPU poll no-op. Source `device.rs:950–986`: ErrorScopeGuard !Send/!Sync; explicit pop возвращает future, drop теряет ошибки. `device.rs:676`: public destroy.

Ссылки на pinned первичные rustdoc: [Queue30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/struct.Queue.html), [Device30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/struct.Device.html), [ErrorScopeGuard30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/struct.ErrorScopeGuard.html). Online30.0.0 docs сверены Keenable, критические детали подтверждены локальным30.0.1 source. Не предполагать что completion callback содержит serial/error: serial захватывает FLUI registration, diagnostics отдельны. Гарантии exact memory release после Device::destroy не проверены и не выводятся из короткого API description.
