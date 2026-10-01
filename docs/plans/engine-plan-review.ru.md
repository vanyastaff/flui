# Критическая проверка и исправление плана engine

Дата: 2026-10-01. Исторический снимок review до реализации: план был исправлен,
foundation migration на тот момент ещё не была реализована. Текущее состояние
описано в [плане внедрения](engine-foundation-implementation.ru.md) и
[измерениях реализации](../research/engine-foundation-measurements.ru.md).
Ранее выполненные audit fixes и demonstrations не являются
доказательством новых lifecycle/color/clip guarantees.

## Вердикт

Исходный план нельзя было реализовывать буквально: атомарная граница IR
противоречила детализации, limits появлялись после новых allocations, CPU finish
смешивался с GPU retirement. Эти ошибки могли закрепить именно тот долг, который
миграция должна устранить. Исправления включены в
[master](engine-foundation-implementation.ru.md),
[IR](engine-frame-ir-migration.ru.md) и
[resources](engine-resource-contract-migration.ru.md).

Три независимых adversarial review дали 24 замечания, включая пересечения:
[correctness](engine-plan-review-correctness.ru.md),
[recovery/security](engine-plan-review-recovery.ru.md),
[API/architecture](engine-plan-review-api.ru.md). Проверены code references и
конкретные failure sequences; будущие ошибки обозначены гипотезами. Review
файлы сохраняют первоначальные замечания, исправленные планы — принятые решения.

## Решения, меняющие порядок реализации

| Атака на исходный план | Принятое исправление | Доказательство при реализации |
|---|---|---|
| Gradient fix, all-family ordering и seal можно выпустить отдельно | Одна атомарная foundation delivery, локальные commits допускаются | Все ordered пары primitives, nested consumers, old category path удалён |
| Immutable stops решают lifetime целиком | Freeze всех per-use bindings, включая viewport/target uniforms | Два viewport flush до одного submit: независимые samples или typed refusal |
| Много маленьких buffers проходит device limit | Admission до CPU growth/preparation, count+peak-byte caps | Exhaustion/refusal, preserved complete target, следующий малый frame |
| CPU finish освобождает budget | Submission owner держит charged reservations до completion/epoch teardown | Delayed completion блокирует admission и затем освобождает его |
| Arc lease гарантирует внешнюю Texture | Managed allocation отдельно от trusted raw import/destroy obligations | Raw misuse failure не объявляется managed snapshot success |
| Scene creation замораживает pixels | SourceScene latest до lowering; lowered lease фиксирует allocation, не texels | Rebind до/после lowering, volatile write и copied snapshot разные rows |
| Producer write сам обновит UI | Content commit revision + dependency damage + realm wake | Unchanged Scene, red→green producer, quiescent host и independent realms |
| Одна effect region достаточно для damage | Input/output/backdrop-read/write-clip dependencies | Whole-target partial vs fresh full, halo вне исходного damage |
| Tagged color можно сразу смешать со старым working path | Source intent + named legacy conversion, затем coherent linear transition | Public consumers, independent blend/filter/gradient/image fixtures |
| Scoped raw queue автоматически безопасна | Sole submit/diagnostics owner, explicit capability trust boundary | Ordering, generation fault mailbox, no false displayed acknowledgment |
| RAII/cancellation автоматически восстанавливают progress | Infallible internal Drops, first failure policy, bounded yielding work | Competing failures, next valid operation, worker/GPU retirement отдельно |
| Crate с try_inverse и зелёные pixels доказывают оптимизацию | Conditioning/projective policy; baseline workloads до performance verdict | Near-singular/w=0/DPR fixtures, measured CPU/GPU/count/payload comparisons |

Per-draw sampling имеет authority; premultiplied opacity масштабирует все четыре
компоненты. Stops >8 не silently truncate. Target origin/DPR/device generation
входят в scope, narrowing проверяется после преобразования. Accepted ADR changes
получают Supersedes/Superseded-by; preserved private invariant — Mapping decision.

Повторная проверка исправленных планов выявила дополнительное противоречие:
сохранение void finish_frame несовместимо с доказанным public in-flight cap,
когда embedder submit выполняет самостоятельно. Решение включено в первый
delivery: engine-owned submission/retirement handoff с немедленной миграцией
Renderer/headless/embedder. При необходимости breaking public hook разрешён,
но только с production callers. Trusted raw submit вне него не покрывается cap.
Конкретная Rust signature определяется после inventory submit sites, до кодирования.

## Что ещё нельзя объявлять решённым

Performance thresholds пока не измерены: baseline workloads и параметры
сопоставления обязательны перед приёмкой migration. Payload accounting не точный
driver VRAM. Raw imports не получают недоказуемой device introspection. Completion
не доказывает scanout. Driver crash, process abort и любой arbitrary Drop нельзя
универсально contained обещанием Result/RAII. HDR metadata не HDR display support.

Пятилетний горизонт проверяет способность контрактов расширяться для 3D, AI,
wide-gamut, accessibility и automation. Он не оправдывает универсальный backend,
мертвый публичный API или speculative dependencies. Pure wgpu сохраняется;
новый capability внедряется с production consumer и negative/recovery proof.

## Следующая конкретная работа

Зафиксировать baseline workloads и затем реализовать первый ordered IR delivery
в исправленных границах. Минимальные admission/completion prerequisites входят
в него, полный cache ledger развивается далее. Не начинать одновременно color,
clip и producer public API: у каждого своя coherent acceptance boundary, при этом
их необходимые resource/target hooks закладываются в первом engine контракте.

При review не запускались новые failure reproductions для hypothesized viewport,
raw destroy, delayed completion и damage cases. Их нельзя приписывать к уже
прошедшим audit tests; соответствующие matrices обязательны для implementation.
