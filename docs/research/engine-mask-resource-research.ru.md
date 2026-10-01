# Ресурсы clip/effect passes: GPU completion и ограничения

Дата проверки: 2026-10-01. Исследование через Keenable и Firecrawl.
Workspace использует wgpu 30.0; прочитаны страницы версии 30.0.1.
Выводы ниже дополняют существующий DeviceDomain, не объявляют новый lifetime
протокол реализованным. Benchmarks и cross-platform GPU measurement не выполнены.

## Проверенные источники

| Источник | Проверенный факт | Ограничение |
|---|---|---|
| [wgpu Queue 30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/struct.Queue.html) | write_buffer копирует CPU bytes в staging сразу, но GPU transfer начинается при следующем submit. on_submitted_work_done наблюдает завершение ранее submitted работы. Native callbacks требуют submit/poll; callback должен быть коротким. | Не означает завершение scanout и не гарантирует общий предел driver VRAM. |
| [MDN GPUQueue.onSubmittedWorkDone](https://developer.mozilla.org/en-US/docs/Web/API/GPUQueue/onSubmittedWorkDone) | Promise завершается после обработки работы, submitted к моменту вызова; completion можно использовать для throttling. | Независимая проверка queue semantics, web API, страница изменена 2026-05-21. Native poll topology отсюда не следует. |
| [wgpu TextureUsages 30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/struct.TextureUsages.html), [source](https://docs.rs/wgpu-types/30.0.1/src/wgpu_types/texture.rs.html) | TRANSIENT_ATTACHMENT совместим только с RENDER_ATTACHMENT; требует Clear/DontCare и Discard. На платформах без пользы может быть no-op. | Extractor потерял heading TRANSIENT_ATTACHMENT; определение перепроверено в исходнике установленного wgpu-types 30.0.1. |
| [Chrome 149–150](https://developer.chrome.com/blog/new-in-webgpu-149-150#stricter_validation_for_transient_attachments) | Transient attachments имеют дополнительные ограничения viewFormats/view usage и не являются resolveTarget. | Документ Chrome/Dawn, обновлён 2026-06-17. Описание tile-memory выгоды не гарантирует отсутствие VRAM затрат на каждом backend. |

Попытка targeted extraction из полного W3C WebGPU вернула пустой ответ и
предупреждение об обрезке страницы. Этот ответ не используется как normative
доказательство. Queue completion перепроверен по wgpu и независимой MDN;
transient usage — по документации и реальному исходнику зависимости плюс Chrome.

## Следствия для FLUI

**CPU scope не является GPU lifetime.** Recording lease проходит через encoding,
submission и completion. Возврат временной mask в pool при Drop dispatcher сам
по себе не разрешает reuse; два encodes до submit особенно различают эти стадии.
DeviceDomain уже учитывает submitted retention: новые маски и intermediates
должны использовать его, а не вводить второй счётчик или глобальный queue owner.

**Submission pacing не требует блокировать каждый кадр.** Completion callback
обновляет bounded bookkeeping и передаёт сигнал инфраструктуре; frame path
остаётся синхронным. При отказе admission native nonblocking polling сохраняет
progress. Нельзя ждать all-work-done только ради readback одного buffer:
mapAsync может завершиться раньше несвязанной работы очереди.

**Transient attachment — конкретная оптимизация, не способ удешевить любую mask.**
Stencil/depth или MSAA attachment, который очищается, используется в одном pass
и discard-ится, может подходить. Mask texture, sampled следующим pass, blur
intermediate, retained target и resolve destination не подходят. До включения
проверять usage, load/store policy и возможности целевого backend; измерять
bandwidth и GPU time на мобильных/Apple и desktop отдельно.

**Ограничивать payload и работу отдельно.** Checked texture extent × layers ×
sample count × format bytes позволяет ограничить известный payload, не всю
driver allocation. Metadata, path flattening, clip depth, passes, staging и
submitted backlog требуют своих счётчиков. Backend allocation failure остаётся
typed failure даже при успешном admission. Для AI-generated сцен это обычный
недоверенный input, а не основание для unlimited geometry work.

**Проверять достижимость новых API, а не только release news.** Chrome 149–150
также описывает immediates: маленькие параметры следующего draw без отдельного
uniform buffer. В установленном wgpu 30.0.1 есть
[RenderPass::set_immediates](https://docs.rs/wgpu/30.0.1/wgpu/struct.RenderPass.html#method.set_immediates),
feature IMMEDIATES, nonzero max_immediate_size и alignment 4. Однако реальный
[web backend source](https://docs.rs/wgpu/30.0.1/src/wgpu/backend/webgpu.rs.html)
в этом release в реализации set_immediates вызывает panic вместо JS forwarding;
документация feature ещё называет browser support будущим. Это перепроверено по
установленным api/render_pass.rs, backend/webgpu.rs и wgpu-types features.rs.
Browser capability и поддержка Rust backend — разные этапы. Для FLUI это
кандидат native fast path для небольших run constants, только с feature/limit
admission и immutable-buffer fallback; не основание заменить portable uniform
path. Скорость, web wiring и accounting costs не измерены.

## Приёмка и измерения

- Вложенные masks, два recordings до submit, refusal после intermediate submit,
  следующий успешный кадр: pixels и ledger recovery должны наблюдаться вместе.
- Удаление CPU scope не уменьшает submitted charge до completion; cancellation
  до submit освобождает несданные ресурсы. Первый failure остаётся authoritative.
- Сравнить transient и обычный attachment при одинаковой quality и pass topology;
  измерить peak charged bytes, staging bytes, pass count, CPU record/encode и GPU
  time p50/p95/p99. Не называть charged bytes измерением фактической VRAM.
- Capability fallback сохраняет результат либо возвращает typed unsupported;
  неизвестный effect не превращается в unmasked content.

Это требования к следующей реализации. Источники не дают универсальных значений
memory caps или доказательства самого быстрого clip renderer для FLUI.
