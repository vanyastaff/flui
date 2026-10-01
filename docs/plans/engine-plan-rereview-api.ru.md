# Повторное API review интегрированного engine plan

Дата: 1 октября 2026. Прочитаны актуальные master, integrated review, IR и
resource планы после исправлений. Production не менялся; failure scenarios ниже
не исполнялись. Это unresolved design decisions, не новые воспроизведённые bugs.
Modern library API сверено с ранее прочитанными pinned primary sources.

## Закрытие предыдущих замечаний

| Предыдущее замечание | Статус после интеграции | Основание |
|---|---|---|
| Несогласованный первый IR delivery | Закрыто на уровне плана | Master mandatory §2 и IR atomic delivery: все families, seal, один order representation |
| >8 gradient stops | Закрыто | Checked supported limit/error; no silent clamp; отдельный ninth-stop row |
| Raw Texture destroy против lease guarantee | Закрыто | Managed factory отдельно от trusted raw import, caller obligations и cap оговорены |
| finish не GPU completion | Закрыто | Recorded/Submitted/Retired, completion pump, charged reservations и teardown |
| Source color vs coherent linear working transition | Закрыто | Две deliveries, legacy conversion названа, matrix/blend domains и serde/hash validation сохранены |
| glamx inverse ≠ conditioning/projective clipping | Закрыто | Actual consumer prerequisite, f64/unit boundary и near-singular/w-zero policy |
| Accepted ADR silent amendment | Закрыто | Supersedes/Superseded-by; private invariant Mapping decision, Proposed уточняется |
| Performance baseline/thresholds отсутствуют | Закрыто как prerequisite | Workloads/p50/p99/count/payload измеряются до acceptance; численные bounds ещё не получены |
| Public external submit без retirement handoff | Закрыто направление, Rust boundary ещё выбрать | IR lines 103–112: engine-owned hook в первом delivery, production consumers вместе, raw submit вне cap |

[color 0.3.3](https://docs.rs/color/0.3.3/color/) действительно даёт typed
AlphaColor/PremulColor, DynamicColor, conversion/interpolation и gradient ramp;
ICC/PQ/display negotiation ему не приписаны. [glamx 0.3.1 MatExt](https://docs.rs/glamx/0.3.1/glamx/trait.MatExt.html)
действительно имеет try_inverse/SVD, не guaranteed conditioning. glam ^0.33.7
совместим с workspace ^0.33. Новая dependency только с production consumer;
это не утверждение о выполненной компиляции. Vello integration не предлагается.

## 1. Prepared commands и reservations должны передаваться одним ownership value

[IR handoff](engine-frame-ir-migration.ru.md#stop-tables-и-gpu-lifetime) принимает
«подготовленные command buffers и charged reservations», но их связь ещё не
выражена выбранным API. **Гипотеза:** caller передаёт buffers frame A вместе с
reservation frame B либо повторяет handoff; cap учитывает другое generation или
allocation. Engine-owned submit сам по себе не связывает произвольный wgpu
CommandBuffer с ledger. Это новая детализация после правильного submission fix.

**Repair:** consuming opaque PreparedSubmission owns command buffers + charges +
owner/device generation + target identity, private constructors от actual prepare.
Discard и submit потребляют этот же value; Submitted token отдельно не допускает
resubmit. Host-generated producer buffers admitted через явно trusted scoped
composition path, не произвольный Vec плюс caller-reported bytes. Rust форму
выбрать до coding; compile-fail cross-owner/reuse плюс public first consumer.
Existing cube raw trusted mode честно может остаться вне cap, но managed path
не должен возвращать raw destructible ownership token.

## 2. Whole-frame отказ несовместим с ранним submit в тот же visible target

IR сохраняет несколько flush в frame (lines 98–101), а failure contract
(lines 162–165) сохраняет last completed target и запрещает partial output.
**Гипотеза:** первый flush уже submit пишет в retained target, следующий flush
отказывает по admission; old completed pixels перезаписаны, discard последующих
buffers их не вернёт. Отсутствие present не сохраняет retained target contents.

**Repair:** выбрать commit boundary: preflight whole frame до первой visible
write или separate speculative target с commit after successful prepare; ранние
submitted offscreen passes допустимы, visible retained target защищён. Для trusted
raw embedder, пишущего в свой target, explicitly narrower caller-owned atomicity.
Acceptance two flushes с failure между ними, затем capture last complete и
subsequent valid frame. Нельзя обещать универсальный rollback submitted GPU work.

## 3. Ledger scope при двух painters на одном device не определён

[Resource admission](engine-resource-contract-migration.ru.md#admission-и-учёт-ресурсов)
называет единый ledger в GpuResources. Нынешний [painter constructor](../../crates/flui-engine/src/painter/mod.rs)
создаёт GpuResources для каждого painter (line 188), даже при клонированных
Arc<Device>/Arc<Queue>. **Гипотеза:** два windows/embedders разделяют device и каждый
получает полный managed budget; device peak удваивается, callbacks и diagnostics
имеют двух owners. Local cap может быть правильным, но не device-total cap.

**Repair:** explicit domain: owner-scoped per-painter cap с оговоркой aggregate
или shared DeviceOwner budget/submit/diagnostics authority, передаваемый обоим
painters без процесса-global registry. Multi-window quotas и queue serials под одним
device owner; realm wake/focus остаются realm-scoped. Duplicate owner constructor
не должен повторно устанавливать device diagnostics без defined replacement policy.
Acceptance two painters same device, concurrent in-flight charges, close одного,
progress другого; отдельные devices имеют independent generations/caps.

## Вывод для review

Первоначальные восемь attacks закрыты в документе, также исправлено направление
public submission hook. Остались три конкретных выбора формы ownership/commit/
budget domain. Они не требуют переписать architecture или ждать будущих SDK этапов;
их следует закрепить непосредственно в первом delivery contract. Проверки и
измеренные thresholds ещё не выполнены; green current audit не доказывает эти
новые гарантии.
