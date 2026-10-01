# Engine allocation/retirement inventory перед миграцией

Дата 2026-10-01. Read-only source inventory, без сборки/benchmark run. Номера строк относятся к прочитанному checkout и могут сместиться при текущей реализации. Это перечень проверенных production точек, не обещание полного статического coverage: allocation через helpers отдельно раскрыта ниже. Wgpu30.0.1, единственный rasterizer wgpu.

## GPU payload allocations и ownership

| Production site | Scope/частота | Текущий retirement/admission и требуемая граница |
|---|---|---|
| `replay/mod.rs:143,168,177` | viewport uniform, unit quad vertex/index при replay construction | Persistent; viewport queue writes mutable. Frozen per-target replacement включается в PreparedIrBudget |
| `pipeline_set.rs:455` и gradient buffer preparation | gradient table binding при refresh | Первый IR delivery заменяет shared table immutable allocation; зарезервировать table bytes/count + CPU conversion peak до allocation |
| `buffer_pool.rs:208` | vertex/index/instance buffer bucket cache miss | Pool reset делает entries reusable, free eviction byte budget; это не GPU completion ledger. Existing buffers — initial quota exclusion |
| `uniform_pool.rs:93` | unique pass uniform slot при pool growth | UNIFORM/COPY_DST, reset after frame; count/bytes до growth, submitted last use для reuse discipline |
| `effects_pipeline.rs:51` | common effect uniform helper | Через helper обслуживает blur/gamma/mode/morphology/color matrix/advanced blend; считать actual capacity и pass counts, не только вызовы каждого filter |
| `texture_pool.rs:396` | pooled offscreen target cache miss | Payload tracking и max available count, return-on-drop channel. Return не GPU retire; free textures всё ещё owned bytes |
| `texture_cache.rs:354` | standalone decoded image upload cache miss | memory_bytes/eviction есть; transient upload и CPU image ownership отдельны. Atlas path также выделяет texture через atlas |
| `atlas.rs:153` | image atlas page construction/growth | Persistent texture; image copy/upload bytes отдельно. Не считать каждую tile ещё одной GPU allocation |
| `glyph_atlas.rs:271` | glyph page allocation | Mask/color pages; glyph CPU raster/cache work и texture upload bytes. Bindgroup rebuild при page change:453 |
| `retained_target.rs:83` | retained target creation/resize | Existing committed image; candidate/staging replacement добавляет ещё target/copy budget, нельзя clear committed до успешной frame commit |
| `headless.rs:230` | output texture render target | Actual production headless target; size/device bounds проверяются. Scoped capture output, не pool |
| `headless.rs:351` | padded MAP_READ staging readback | Row-aligned copy bytes, CPU output Vec и map lifetime; copy submit retirement отдельно от render submit |
| `external_texture_registry.rs:185,277` | bindgroup на register/update | Texture уже выделена trusted caller; registry не предотвращает external allocation. Descriptor/views/metadata count admission, managed factory отдельная ownership гарантия |
| `offscreen/mod.rs:301,318` | cached fullscreen quad и blur uniform pool growth | Persistent/pooled; per-pass unique slots до submit. Не смешивать с STORAGE table arena |
| `offscreen/blit.rs:155` | cached blit fullscreen vertex buffer | Cold allocation; per-use bindgroup:211. Texture pool targets через caller |
| `offscreen/mask.rs:188` | mask parameter buffer на operation | Transient; bindgroup:199 и submit:263. Требуется intermediate submission ownership |
| `ssaa.rs:204,724` | cached downsample geometry и per-use downsample params | SSAA textures через texture pool, sample region/scale влияет на cost. Per-use binding:732 |

`headless.rs:517,639`, renderer allocations после cfg(test) suites, `advanced_blend/mod.rs:374,422,587,629,665`, `test_support.rs` и `*_tests.rs` относятся к тестовым/readback paths: не включать их количество в production per-frame baseline. Источник shader/pipeline allocations тоже GPU overhead, но их driver bytes неизвестны; отдельные count/cold-compilation metrics нужны вместо ложного payload estimate.

## Binding/pipeline counts, которые нельзя потерять

`replay/flush.rs:712,1063,1175` создаёт per-flush bindgroups. `external_texture_registry.rs:94,119,130` — registry layout/samplers при construction. `offscreen/blit.rs:27,98,140`, `offscreen/mask.rs:49`, `offscreen/blur.rs:58,107` — lazily cached layouts/pipelines/sampler. `pipeline_cache.rs:497,604` — state-driven cache miss. `pipeline_set.rs:303,538,556,598,688` — fixed layouts/pipelines. Filter `*/pipeline.rs` и sampler `*/mod.rs` используют существующие shared builders. Эти objects учитываются count/work quota, не выдуманными «4 bytes/pixel».

## Реальные production submission boundaries

- `renderer.rs:2335`: FrameRasterOps submission; `renderer.rs:2444` clear и `:2519` final submission в capture/frame helper path. Каждый call-site нужно классифицировать по caller, не переносить только final encoder.
- `layer_dispatcher.rs:434`: ранний painter flush перед effect work.
- `offscreen/blur.rs:211,421`, `offscreen/mask.rs:263`, `offscreen/blit.rs:256`: промежуточные submits operation.
- `headless.rs:224,324,389`: render/common raster helper/readback copy соответственно.

Текущее `offscreen/mod.rs:123` «submit once at end» нельзя использовать как proof: actual paths выше отправляют несколько submissions. Recording failure после одного из них даёт PartialSubmitted: отправленные charges retire только по completion, остаток discard. Candidate target сохраняет предыдущий committed image. Shared resource, используемый несколькими submissions, сохраняет reservation до последнего admitted use, а не первого callback.

## Уже доступные метрики и ограничения

`texture_pool::PoolStats` (`texture_pool.rs:151–158,358`) даёт allocated/available и approximate total_memory_bytes. `texture_cache.rs:410` суммирует standalone memory_bytes. `buffer_pool.rs:338,357` даёт capacity и pool stats. Tracing cache-hit/create/return events есть в texture pool, cache и buffers, renderer — damage/present events. Они не образуют единого live+submitted peak ledger и не измеряют driver VRAM.

`frame_timing.rs` — portable monotonic diagnostic timestamps. Optional `gpu-profiler` feature использует wgpu-profiler; `profiler.rs:148,226` требует resolve_queries до последнего submit. GPU timestamps условны по adapter capabilities; нельзя включать mandatory feature на неподдержанном adapter. Criterion измеряет CPU-observable iterations, обычно с device.poll wait; это не автоматически отдельный encode p99 или GPU execution distribution.

## Маленький воспроизводимый baseline через existing benches

Команды запускает единственный compiling worker после текущего gate. `--quick` — короткий smoke comparison, не statistically stable p99. Один adapter/build/profile, одинаковая scene, warm/cold отдельно; сохранить stdout и criterion artifacts. Команды ниже не выполнялись.

```powershell
cargo bench -p flui-engine --features testing --bench render_throughput -- painter_render_50rects_gradient_text --quick
cargo bench -p flui-engine --features testing --bench offscreen_resource_cache -- render_masked_256x256_solid --quick
cargo bench -p flui-engine --features testing --bench offscreen_resource_cache -- render_blur_256x256_sigma5_3passes --quick
cargo bench -p flui-engine --bench text_throughput -- steady_state --quick
cargo bench -p flui-engine --bench raster_backpressure -- submit_pump_retire_cycle --quick
```

`render_throughput.rs:228` включает fill draw-list→encode→submit→wait GPU roundtrip, 50 rects+gradient+text; не чистый record timer. `offscreen_resource_cache` bench прямо отделяет CPU resource overhead от GPU time; counts и cold object microbench `eliminated_allocation_overhead` полезны вторым этапом. `text_throughput` содержит steady_state/cold_rows; первый quick bounded, cold rows отдельно показывает atlas churn. `raster_backpressure` NoOpBackend не GPU ledger: только mailbox/backpressure control baseline. Existing damage benchmarks можно выбрать по имени `damage_retained_target/blit_only`, но они не заменяют alternating-gradient and heavy nesting workload, отсутствующий в этой baseline выборке.

## Минимальные production шаги DeviceDomain / PreparedIrBudget

1. Private instance-owned DeviceDomain: device epoch, sole submission/diagnostics registry, bounded completion mailbox, realm quota/wake ownership. Shared painters получают тот же Domain, а не новый cap на одну Device. No process-static state.
2. PreparedIrBudget для **новых** immutable stop/viewport/instance allocations и CPU temporaries: reserve checked peak+count до construction, rollback infallible. Explicit exclusions до полного ledger: existing image/cache/atlas/pool/buffer allocations и external raw allocations; измерять их отдельно. Не объявлять whole-engine cap.
3. Opaque consuming PreparedSubmission объединяет encoded command buffers, owner/epoch/target и charges. Мигрировать перечисленные production submits вместе; PartialSubmitted owner не отдаёт charges при позднем Err. Callback короткий generation/serial message, без сильного cycle к Domain/Device.
4. PreparedFrame target staging/copy preflight; reserve double target если нужен rollback. Commit меняет только completed candidate reference; discard не разрушает прежнее изображение. Known requirements preflight до первого submit, GPU delayed failure policy отдельно от CPU validation.
5. Closing/Draining/Lost pump переживает окна: bounded nonblocking native poll, browser progress, deadline≠completion. ErrorScopeGuard push/pop на operation thread с reverse explicit pop даже early failure; futures edge resolver, first error authoritative. Raw Device/Queue остаются trusted mode.
6. Baseline+negative tests до claim: delayed completion, failure после intermediate submit, two managed painters, late loss/close callback, budget fail→next valid frame. Затем распространять allocation ledger на pool/cache/atlas и lifetime categories без double counting.

Evidence gathering: `rg -n 'create_buffer|create_texture|create_bind_group|create_sampler|create_render_pipeline|queue.submit' crates/flui-engine/src -g '*.rs'` с чтением cfg и caller sites; `rg --files crates/flui-engine/benches`; manifest required-features и конкретные bench_function имена проверены. Global counts нельзя вывести из grep matches: loops/cache misses и helpers меняют actual frequency. Actual peak/readback/performance значения ждут запуска parent worker.
