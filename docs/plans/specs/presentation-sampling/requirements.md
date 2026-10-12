# Presentation sampling: failure provenance, healthy continuation and recovery

**Status:** Approved by the maintainer on 2026-10-11. Implementation in progress; acceptance remains subject to the specified tests, migration and review.

**Issue:** [#1552](https://github.com/vanyastaff/flui/issues/1552). **Date:** 2026-10-11. **Evidence baseline:** `6af2cf426809dbf8a33bbb415e904f0bfdc847c4`.

## Problem Statement

Автор приложения ожидает, что ошибка анимации или gesture deadline одной presentation не лишит здоровые presentations уже принятой работы. Сегодня runtime sampling находится вне presentation-local frame containment: первый вышедший panic может прервать sampling следующих presentations.

При этом ошибка вычисления и ошибка доставки имеют разные последствия. Curve или simulation может упасть до принятия нового sample; listener может упасть после корректной публикации. Текущие animation/gesture entry points возвращают только управление либо opaque first panic. Runtime не получает доказанного происхождения ошибки и не может безопасно выбрать продолжение по panic text или текущему значению controller.

Особенно опасен смешанный случай: первая ошибка — listener delivery, следующая — source computation. Первую причину нужно сохранить, но более поздняя ошибка всё равно делает sampling cohort непригодным для нового кадра. Простое правило по первой ошибке даст неверную публикацию.

ADR-0048 явно исключает prephase из принятого containment. Эта спецификация расширяет контракт, а не объявляет существующую реализацию нарушением прежнего обещания.

## Solution

Runtime исполняет sampling как одну presentation-owned операцию. Она различает доказанные ошибки вычисления, ошибки доставки после commit и случаи неизвестной целостности, сохраняет первое failure отдельно от пригодности всего cohort и выполняет независимую здоровую работу. Восстановление включает владение, retained text, accepted delivery, deadlines, frame demand и следующую операцию.

Новая scene не публикуется для presentation с неуспешным вычислением или недоказанной целостностью. Доказанная delivery-only ошибка сохраняет опубликованный sample и позволяет обычное продолжение, если не возникла другая блокирующая ошибка и presentation остаётся пригодной к исполнению. Все здоровые presentations получают выбранное время pump и продолжают существующий frame path.

## User Stories

1. As an application author, I want healthy presentations to receive their accepted sample after a sibling fails, so that one callback cannot starve unrelated UI.
2. As an application author, I want healthy eligible presentations to continue build and completion, so that catching an error does not merely postpone the same interruption.
3. As an animation author, I want a failed source to preserve the last published value and its timing, so that a rejected proposal cannot advance its derivative.
4. As a listener owner, I want a delivery error to preserve the already committed sample, so that observer failure does not masquerade as failed computation.
5. As a listener owner, I want the remaining live listeners and admitted completions to be attempted, so that my work survives another listener's failure.
6. As a runtime consumer, I want the first failure preserved when a later operation fails, so that recovery cannot replace the original cause.
7. As a runtime consumer, I want later computation failure to prevent publication even after earlier delivery failure, so that first-error preservation does not hide an invalid cohort.
8. As an extension author, I want unknown commit state identified explicitly, so that the runtime never pretends an arbitrary callback completed safely.
9. As an input consumer, I want independent deadlines to progress after animation failure, so that visual work cannot erase input correctness.
10. As an input consumer, I want a committed long-press transition delivered without repeated activation on the next pump, so that containment preserves contact lifecycle.
11. As an extension author, I want failing deadline discovery to preserve healthy deadline sources and recovery wake, so that the owner loop survives beyond the tick callback.
12. As an application author, I want closing or replacing authority to suppress obsolete work, so that recovery cannot revive a retired presentation or contact.
13. As an application author, I want reentrant admissions to follow existing generation and ordering rules, so that one failure does not duplicate or reorder work.
14. As an application author, I want retry demand to survive a healthy sibling's completion, so that accepted recovery work is not cleared by mark-rendered bookkeeping.
15. As an application author, I want a repeated deterministic failure to use existing pacing, so that recovery does not become an unbounded retry loop.
16. As an application author, I want fixing a source or listener to permit the next operation to progress, so that containment does not permanently disable the driver.
17. As a text consumer, I want compatible retained preparation to keep its frozen numerical tick after delivery-only failure, so that geometry and animation never refer to different attempts.
18. As a text consumer, I want obsolete or untrusted retained preparation withdrawn safely, so that a late reply cannot publish rejected geometry.
19. As a completion consumer, I want callbacks to become eligible only after valid terminal completion, so that failed or waiting attempts cannot falsely complete.
20. As a runtime consumer, I want typed failure origin, operation and address, so that diagnostics do not require parsing logs.
21. As a runtime consumer, I want delivery-only recovery to leave the frame-failure streak unchanged, so that observers do not accidentally trigger frame escalation.
22. As a runtime consumer, I want a rejected attempt counted once even with competing failures, so that escalation measures attempts rather than exception count.
23. As a runtime consumer, I want report handlers and tracing failures contained independently, so that diagnostics cannot interrupt healthy presentations.
24. As an owner, I want captures, sources and opaque panic payloads retired under established custody, so that nested cleanup cannot replace the first failure or abort avoidably.
25. As a scheduler consumer, I want muted, hidden and retained presentations to respect existing time and eligibility rules, so that recovery does not introduce extra samples.
26. As a standalone animation or gesture consumer, I want existing propagation semantics retained, so that runtime integration does not silently change independent callers.
27. As a Rust contributor, I want one coherent sampling operation with typed outcomes, so that callers do not assemble catch order, report severity and retry bookkeeping manually.
28. As a maintainer, I want public-path regression and recovery evidence, so that a green helper test does not conceal an unwired production path.
29. As a cross-platform application author, I want logical containment guarantees distinguished from native execution evidence, so that support claims are accurate.
30. As a contributor, I want the new contract linked to its partial ADR supersession, so that old exclusions and current code cannot contradict each other.

## Implementation Decisions

Все решения ниже предлагаются к утверждению вместе; названия конкретных новых Rust items уточняются при реализации в этих границах.

### Provenance and publication

| Category | Required evidence | State and frame outcome |
|---|---|---|
| Computation failure | Escaped failure при вычислении требуемого proposal до valid commit | Неуспешный proposal не публикуется; его прежние value/timing сохраняются; admitted attempt этой presentation не исполняет новый segment; `FrameDropped` |
| Delivery failure | Framework transition, valid sample и delivery debt уже committed до failing callout | Published state сохраняется; healthy accepted tail исполняется; segment может продолжиться после обычной проверки premises; `Contained`, если cohort не инвалидирован другой ошибкой |
| Unknown consistency | Open extension или preparation/query/retirement failure не имеет доказанного commit/postcondition | Не выдавать такой исход за computation либо успешную delivery; консервативно не публиковать новый segment admitted attempt этой presentation; `FrameDropped` с явным unknown origin; без attempt — contained operation report |

- Origin фиксируется на настоящем source/commit/delivery seam до user code. Нельзя восстанавливать его из `BUG:`, panic text, равенства значений, cleared deadline или состояния, прочитанного после reentry.
- Первое failure и aggregate publication eligibility — независимые факты. Любая более поздняя computation/unknown ошибка запрещает segment, сохраняя исходный payload и origin. Факт, почему cohort был rejected, доступен в typed diagnostics.
- Reentrant supersession, intentional mute, removed-before-turn member, closed authority и документированное typed refusal сохраняют собственные domain semantics. Они не превращаются автоматически в panic или dropped attempt.
- Existing handled non-finite source conditions не включаются в новые failures: time/curve fallback сохраняет предыдущий sample/run, custom simulation fallback завершает run на последнем finite value и settles future по действующему контракту. Их предупреждения и outcome не меняются. Panic самой source или diagnostics требует собственного origin; нельзя переписать эту domain policy вместе с containment.
- Для built-in producers классификация опирается на реальные commits. Open `GestureArenaMember` без такого протокола получает unknown consistency; эта задача не требует массовой замены gesture API.
- `FrameDropped` относится только к admitted fresh/retained frame attempt. Для hidden, paced или иначе ineligible presentation без такой попытки fault создаёт contained operation report и recovery debt, но не fictitious dropped-frame streak. Незавершённая retained attempt является принятой работой; её rejection/withdrawal учитывается один раз.

### Ownership and orchestration

- Операция принадлежит `flui-runtime`, а provenance возникает в `flui-animation`/`flui-interaction`. Native host, window и GPU ownership не переходят в runtime.
- Все sampling stages всех presentations выполняются до любого build/segment. Pump выбирает raw host time один раз; presentation clocks применяют текущие motion policies.
- После sampling один раз принимается fresh-attempt eligibility/admission decision по действующим demand, gates и FrameClock rules, до sampling reports. Retained attempt сохраняет собственную identity. После diagnostics перепроверяются authority/premises, но FrameClock не poll-ится второй раз и counted rejected outcome не переопределяется задним числом. Hidden без retained attempt остаётся no-attempt operation.
- Ошибка animation не отменяет независимые gesture deadlines той же живой presentation. Ошибка одного member не теряет live tail остальных. Close/withdrawal/mute/generation authority перепроверяются по существующим правилам перед работой.
- Сохраняются admission fences, controller epochs, single-parent Vsync, cancellation, registration order и FIFO debt. Нижние standalone entry points сохраняют документированную propagation; outcome-producing integration имеет настоящий runtime consumer.
- Используется существующая failure custody. Не вводить второй глобальный catch framework, process-wide sampling state, public test-only seam или новый crate.

### Recovery, retained text and wake

- При computation/unknown failure неуспешный producer сохраняет свой прежний published sample; уже committed samples здоровых controllers этого cohort остаются. Новый scene candidate/completion rejected presentation не публикуются, прежняя presented scene сохраняется. Это не rollback всего cohort или произвольных side effects приложения.
- Compatible retained text после delivery-only ошибки сохраняет frozen tick, accepted frontier и receipts. После sampling всех siblings выполняется повторный reconciliation; изменённые premises withdraw попытку и требуют fresh sample следующего pump.
- Withdrawal retained preparation — целостный переход: old receipt identity инвалидируется, geometry отзывается, layout получает fresh runnable debt, commit/document barrier остаётся uncommitted до coherent replacement attempt. Простого удаления saved segment недостаточно. Late settlement старого text-sizing receipt возвращает `false`; новое work позже может settle и завершить segment. Native work может физически закончиться, но не авторизует withdrawn attempt. Unknown consistency требует такого withdrawal; остальные premise-invalidations сохраняют действующий proof-based reconciliation.
- Recovery demand и animation continuation остаются owed после общего idle/mark-rendered clear. Следующая healthy операция может завершить runnable frame/layout work без нового внешнего input. Repeated frame failure использует действующую pacing/escalation policy; автоматическая остановка controller, общий frame/controller circuit breaker и немедленный retry-loop не вводятся. Отдельная deadline quarantine описана ниже.
- Deadline discovery и wake folding входят в границу recovery. Open queries исполняются при owner execution, без registry/AppRuntime guards. Safe scheduling snapshot обновляется после admission/input/deadline/replacement/withdrawal и owner turns с build, finalization, completion, async/background work или deferred text grants. Host-facing wake/pending getters читают только snapshot: без foreign calls, tracing, destruction и повторного выполнения query.
- Runtime-hosted external deadline source получает узкую owner-local update authority: публикация нового срока или cancellation автоматически invalidates scheduling observation и wakes owner. Более ранний срок нельзя сообщать лишь скрытой мутацией external Cell. Built-in producers и runtime extension integrations мигрируются на этот путь; stale/withdrawn authority refused по identity. Standalone live-query API сохраняется. Это narrow scheduling contract, без нового универсального recognizer framework.
- Query или untrusted poll fault не обрывает healthy deadline sources. Он сохраняет одну generation-fenced unresolved obligation; прежняя просроченная contribution этого source не удерживает hot polling. Чистая `deadline()` query получает ровно один automatic re-probe не раньше 100 ms host time; success восстанавливает observation, повторный fault паркует debt до explicit update/new revision. Getter и earlier healthy turn не выполняют этот probe досрочно, не сдвигают due time и не создают повторного report.
- Unknown `poll_deadline()` не replay-ится автоматически для той же contact/deadline revision: side effects могли уже произойти. Ambiguous obligation parked, не completed и не silently cancelled. Recovery требует явно принятой new revision/rearm либо retirement; новая revision supersedes прежнюю через действующую custody, не объявляя её задним числом выполненной. Quarantined contribution не блокирует healthy deadline fold или fresh coherent geometry.
- Новая deadline-update authority fenced по source/contact/revision, не переиспользует exhausted identity и позволяет явный refresh/rearm даже при том же численном сроке. Гарантия frame retry без нового external input относится к runnable frame/layout work; она не обещает автоматический replay неизвестного user operation. Deadline arithmetic checked; terminal clock не выдаётся за успешно armed future wake.
- Close/withdrawal удаляет contribution по identity. Suspended host сохраняет resume semantics, без обещания OS execution в фоне. Bounded query re-probe — logical input-recovery policy FLUI, не native compositor fallback или W3C threshold. Первое failure этой operation остаётся authoritative; unresolved obligation диагностируется один раз, retry query не повторяет её report или dropped-attempt count.
- Вне frame attempt query fault получает queued addressed contained operation report без streak increment. Delivery ожидает безопасной owner boundary после освобождения guards/checkouts; принятие report debt предшествует wake. Fault не вызывает диагностику внутри wake getter или рекурсивную query/report loop.
- Pending native text frontier остаётся нетерминальным ожиданием; для него не выдумывается clean frame, completion или failure-streak reset.

### Reports and exceptional values

- Один sampling cohort выдаёт один authoritative aggregate report в порядке presentation admission. Report различает affected operation, first origin, blocking cohort evidence и наличие admitted attempt; `FrameDropped` инкрементирует streak ровно один раз, delivery-only/no-attempt `Contained` не инкрементирует и не сбрасывает его.
- Sampling reports delivered после завершения всех sampling stages и установления recovery state, до build walk. После diagnostics выполняется reconciliation retained premises, чтобы новые invalidations не публиковали obsolete geometry. Handler contract явно описывает этот новый sampling boundary; остальные lifecycle/segment reports сохраняют существующие timing. Queued outside-frame operation reports delivered только на безопасном owner boundary, без caller-held host guards.
- Последующий terminal clean frame сбрасывает streak по действующему контракту. Другие recovered lifecycle/segment reports сохраняют свою существующую семантику и не дублируют sampling failure.
- Recovery state, debt и payload custody установлены до diagnostics. Report delivery выполняется без storage guards; subscriber, handler и их retirement содержатся независимо. Одна попытка report delivery, без повторного исполнения самого ошибочного callback.
- `BUG:` остаётся диагностической metadata, не переключателем policy. Для новых sampling/operation panic messages redaction одинаково применяется к tracing и handler; opaque payload не публикуется как user-visible DTO. Existing Pipeline reports сохраняют typed `RenderError` в handler: эта policy не меняется.
- Здоровые destructors исполняются нормально. После failure outgoing opaque ownership и competing payloads сохраняются по действующей exceptional-retention policy. `panic=abort`, OOM abort и aggregate double-panic до boundary остаются явными пределами.

### Compatibility and lasting decisions

- Runtime diagnostic surface получает явный sampling/operation origin вместо фиктивной Build phase или outside-frame callback attribution для ошибки внутри frame.
- Public reporting additions имеют production caller; все exhaustive consumers, SDK surface, docs и examples мигрируются в том же пакете. Проверяются additive compatibility и SemVer; намеренный break не прячется под refactor.
- Новый ADR частично supersedes исключение prephase ADR-0048, сохраняя segment recovery, close policy ADR-0123, first-failure custody и standalone delivery contracts. ADR number назначается при реализации после сверки актуального каталога.

## Testing Decisions

Основной seam — публичный `UiRuntime::pump` с mounted stateful widgets, lifecycle-acquired Vsync scope, реальными controllers/recognizers, manual clock, addressed failure handler и существующим recording sink. Synthetic scheduling helpers не должны сами создавать проверяемую dirty/retry обязанность. Private seam допустим только для недостижимой легальным caller терминальной failure competition.

| Scenario family | Observable acceptance |
|---|---|
| Healthy control, source curve/custom simulation failure | Sibling получает именно выбранный tick; rejected source не меняет published timing; failed presentation не публикует scene/completion; исправленный source проходит следующий pump |
| Value/status/completion delivery failure | Valid sample виден, healthy tail исполнен, eligible same presentation и sibling продолжают; report `Contained` |
| Delivery → computation, computation → delivery | Первый origin/payload одинаково authoritative; обе mixed попытки дают один dropped outcome; healthy work продолжает |
| Built-in long press и independent deadline | Committed contact transition не повторяется; healthy notices/deadlines и siblings исполняются; reuse pointer ID не доставляет старую обязанность |
| Open deadline poll/query failure | Unknown origin; hidden/no-attempt даёт operation report без dropped streak; admitted/retained fault withdraw локально; query получает один 100 ms probe, unknown poll ждёт explicit rearm; healthy deadlines сохраняются |
| Retained text и callback-driven invalidation | Compatible tick/frontier frozen; old settlement возвращает false после withdrawal; fresh layout runnable без нового внешнего input; barrier не показывает ложный Committed; replacement settle завершает ровно одну completion |
| Reentry, replacement, closure, mute | Неправомочная старая работа не исполняется; live tail сохраняется без дублей; intentional cancellation не маскируется под fault |
| Retry and streak | Healthy sibling не стирает recovery demand; repeated rejected attempts считаются по одному; delivery-only не меняет streak; clean terminal completion сбрасывает его |
| Callback + report + Drop competition | Original failure/custody сохранены; report attempt один; healthy sibling и следующая операция достигаются; aggregate abort cases изолированы subprocess |
| Gates and standalone consumers | Hidden/muted/retained rules сохраняются; deadlines независимы; standalone propagation и existing healthy tails не меняются |

Дополнительные controls: healthy controller commit перед failing source сохраняется; handled non-finite source outcomes/futures неизменны; deadline reads и earlier healthy turn при 99 ms не повторяют query, owner turn при 100 ms исполняет единственный re-probe. Повторный query fault не создаёт ещё automatic wake. Unknown overdue poll со счётчиком side effects вызывается один раз для этой revision; новая принятая revision может прогрессировать. Earlier healthy deadline не задерживается; earlier deadline/cancellation обновляет snapshot, stale publisher не изменяет новый owner. Реальный owner/native wake-fold seam проверяет отсутствие foreign calls под внешними guards.

Текущий sink обслуживает одного selected producer. `FrameOutcome::presented()` не является per-presentation proof: здоровые siblings проверяются настоящими builder/completion observations, retention scene — recording sink primary. Перестановка mount order не требует двух native surfaces.

Существующий RED доказывает только пропуск sibling sampling после listener/long-press panic. Он не доказывает proposed classification, publication и report outcomes. Для каждого нового обещания нужен decisive baseline RED и counterfactual revert после fix. Уже работающие published-value/registry-tail invariants остаются passing controls. Присутствие widget или равенство private flags не является приёмкой.

Targeted tests, doctests, optional features и consumer compilation выполняются по фактической migration; один финальный scoped gate после последнего изменения. Hardware pen/touch, browser IME, OS compositor и mobile lifecycle не доступны как execution proof в этом synthetic этапе. Linux/native и cross-target CI результаты фиксируются отдельно от локальных Windows проверок.

## Out of Scope

- Parallel execution, одновременно paintable Shared native windows, новый multi-surface sink и обещание performance isolation между presentations.
- Rollback arbitrary app state, подавление invariant ошибок, восстановление после process abort или автоматическое исправление пользовательского source.
- Изменение gesture vocabulary/recognizer algorithms, motion precision, завершённых controller ownership fixes и уже слитого host-delivery пакета. Narrow runtime deadline-update authority и migration его producers входят в scope этой спеки.
- Новый instrumentation framework, telemetry backend, общий frame/controller circuit breaker, blanket controller disposal и изменения CI workflows. Парковка конкретной unknown deadline obligation входит в bounded recovery этой спеки.
- Остальные audit findings не реализуются скрыто внутри sampling. Они сохранены в отдельном follow-up register и пакете ADR hygiene.

Отдельная работа остаётся явной: native Web key/TSF/AppKit mapping; принятые ADR0142 composition exits и anchor debt; reactive identity и AssetKey admission; MIME preservation; directional accessibility/locale policy; callback retirement/reentry; CLI process/NDJSON и tooling classification; первые real consumers и Proposed SDK/capability/raster/persistence scopes. Sampling merge не закрывает эти области.

## Further Notes

Основания: [HTML animation callbacks](https://html.spec.whatwg.org/multipage/imagebitmap-and-animations.html#run-the-animation-frame-callbacks), [Web IDL report behavior](https://webidl.spec.whatwg.org/#invoke-a-callback-function), [Flutter accepted callback-state repair](https://github.com/flutter/flutter/pull/91792), [Flutter active-animation ownership repair](https://github.com/flutter/flutter/pull/125721), [React callback provenance rationale](https://github.com/react/react/issues/11409#issuecomment-340859253), [Rust unwind limits](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html).

Источники обосновывают healthy continuation, определённый postcondition и наблюдаемость failure. Они не задают универсальный native `FrameDropped` rule. Здесь granularity и disposition выбраны по ownership/commit model FLUI и требуют собственных tests.

The maintainer approved the behavior table, unknown-extension fallback, retained/wake recovery and public testing seam on 2026-10-11. Repository requirements/design/tasks track implementation and acceptance.

Related specification: [ADR authority, navigation and obsolete records](https://github.com/vanyastaff/flui/issues/1553). ADR hygiene has its own approval and implementation scope.
