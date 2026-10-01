# Engine submission/write inventory перед ordered IR migration

Дата: 2026-10-01. Статический inventory текущего dirty checkout; parent параллельно
владеет IR работой, поэтому номера строк являются baseline на время чтения.
Сборки и production edits не выполнялись. Прочитан flui-engine ARCHITECTURE,
включая record/replay, offscreen и frame protocol. Architecture схема говорит
«one queue.submit»; фактический window frame содержит несколько submits.

## Метод и границы

Команды: `rg -n 'queue.submit|queue.write_buffer|queue.write_texture' crates/flui-engine/src -g '*.rs'`
и отдельный поиск callers/`#[cfg(test)]` вокруг найденных строк. Совпадение grep
не доказывает production reachability. Особенно важно: все submits
`advanced_blend/mod.rs:587,629,665` находятся внутри тестового module с
`#[cfg(all(test, feature = "testing"))]` в строке321. Production advanced blend
принимает caller encoder; отдельного submit у него нет.

## Submission sites и зависимости

| Callsite | Reachability и работа | Необходимый предшественник / следующий consumer |
|---|---|---|
| `renderer.rs:2444` | production FrameSteps clear target | выбран target; до layer content |
| `renderer.rs:2335` | production handle_shader_mask child clear+render | capture child subtree → child texture готова mask pass |
| `offscreen/mask.rs:263` | production через renderer:2349 | child2335 → masked output → main painter composite |
| `renderer.rs:2519` | production final content encoder | ordered lowered DrawItems и effects → retained blit/present |
| `offscreen/blit.rs:256` | production через renderer:2531 | completed retained content → surface write/clear → present |
| `headless.rs:224` | production HeadlessRenderer scene content | capture target clear/record → readback |
| `headless.rs:324` | compiled clear_to_background helper | clear → subsequent content; retained helper caller scope проверить при миграции |
| `headless.rs:389` | production readback copy | all producer submissions → staging copy → poll/maps |
| `headless.rs:639` | test-only retained-capture FrameSteps, cfg at603 | не отдельный production caller |
| `layer_dispatcher.rs:434` | production apply_backdrop_blur через renderer handle_backdrop_filter:2196 | painter flush → copy surface region → render_blur |
| `offscreen/blur.rs:211` | production sigma<=0 copy через dispatcher:444 | input ready → copied output |
| `offscreen/blur.rs:421` | production Dual Kawase через dispatcher:444 | input → downsamples → upsamples → blur output |
| `advanced_blend/mod.rs:587,629,665` | test-only, не production | synthetic source/target/CPU oracle test |
| `renderer.rs:2636` и submits ниже test section | tests/helpers, не migration production hook | различать по cfg, не mechanical replace |

`render_blur` имеет production caller dispatcher:444, который вызывается
из renderer handle_backdrop_filter:2196, и дополнительный bench consumer.
Первый поиск callers дал неполный результат; исправлено повторным прямым чтением.

Window ShaderMask dependency: child clear/render submit → render_masked submit →
queue_offscreen_result в main painter → final content submit. Backdrop Renderer вызывает dispatcher helper; flush/copy submit434
предшествует blur submit211/421 и последующему main composite. `layer_offscreen.rs` и production `advanced_blend` сами
пишут в переданный encoder; они не создают свой queue.submit.

## CPU writes и resource lifetime

| Callsite | Resource | Policy/опасность при смене submit topology |
|---|---|---|
| `pipeline_set.rs:454` | shared gradient STORAGE offset0 | известный overwrite для нескольких segments до submit; immutable table требуется |
| `replay/mod.rs:217`, caller painter resize:527 | shared viewport uniform | два flush с resize до submit читают последний viewport; freeze per prepared target |
| `uniform_pool.rs:105` | bucket slot per alloc | cursor уникален до frame reset; reset между encoded uses запрещён |
| `buffer_pool.rs:183,214` | reused vertex/index/instance buffers | uniqueness зависит от in_use; `painter/mod.rs:445` reset после каждого render, не submit |
| `offscreen/blur.rs:306` | shared blur slot per mip index | собственный submit позволяет следующий invocation write позже; collapse submits ломает это |
| `glyph_atlas.rs:134` | glyph page texture | slot/frame reclamation must not occur before encoded readers assigned ownership |
| `atlas.rs:284,313,343` | image atlas texture pages | repeated writes to same pixels не создают snapshots; allocation/repack/UV lifetime audit обязателен |
| `texture_cache.rs:370` | decoded image GPU texture | upload before consuming submission; payload/staging reserved before upload |
| `headless.rs:485,531` | test fixtures under cfg(test) | не production texture ingress |

Mask uniform `offscreen/mask.rs:187` создаётся per-call через create_buffer_init;
он не shared offset hazard. Blit fullscreen vertex content и cached pipelines
инвариантны; уничтожение/reuse mutable target — отдельный obligation.
Advanced blend `advanced_blend/mod.rs:250` allocates uniform через существующий
UniformPool; pass копирует backdrop в caller encoder (`:107`, `:187`) перед draw.

Blur `offscreen/mod.rs:113–125` прямо полагается на separate submission каждого
render_blur. Down/up pass reuse объяснён в blur:365: одни src dimensions и offset
на slot в invocation. Эта локальная invariant не доказывает безопасность двух
разных invocations до одного submit. Freeze per encoded use до topology changes.

Особый unresolved path: public multi-flush render_to_view при одном encoder
сбрасывает BufferPool in_use после каждого render. Второй flush может переписать
первый buffer через queue.write_buffer до общего submit. Это конкретный статически
обоснованный сценарий, но GPU test пока не выполнен. При миграции reset должен
переехать в consuming submission/frame ownership или alloc cursor must remain
unique across unsubmitted encoded uses. Нельзя чинить лишь uniforms/stops.

## Errors и rollback

1. `renderer.rs:2332–2335`: ShaderMask child painter.render Err только логируется,
   затем encoder submitted; следующий render_masked продолжает работу. Это
   достижимый production branch, наиболее узкий первый error-propagation fix.
2. `layer_dispatcher.rs:404–434`: backdrop painter.render Err логируется; copy и
   submit продолжаются. Helper достижим из Renderer; branch нельзя переносить
   в новый owner как успешный prepared submission.
3. `layer_dispatcher.rs:342–343,378–387`: bool false/no offscreen/no bound surface
   выражают availability fallback, не typed authoritative frame failure.
   Сначала определить deliberate policy, затем менять return contract.
4. `advanced_blend/mod.rs:201–213`: no region → skip offscreen/zero-area, а не
   поглощённый Result. Не превращать корректный invisible draw в ошибку.
5. `renderer.rs:2506–2511`: final painter Err уже propagates и не final submits;
   однако earlier clear/child/effect work может быть submitted. Finish/discard
   не возвращает committed pixels назад.

FrameProtocol `:175–181` выполняет clear на выбранном retained texture до
fallible content и commit. Existing partial target не transactional last image.
Нужен separately reserved candidate + copy prior committed для partial; failed
candidate не становится committed. Submitted chunks держат reservations до
completion, даже если остальная frame подготовка отказала.

## Минимальное cohesive первое code изменение

Первый узкий correctness hunk — propagate ShaderMask child failure в
`renderer.rs::handle_shader_mask`, через существующий fallible layer visitor /
frame failure context, до child submit и mask invocation. Signature/callers менять
совместно; не добавлять новую публичную hook ради этого. Dispatcher
apply_backdrop_blur получает Result policy при миграции owner, а не silent log.
Failure injection at actual render seam → no success present → next valid mask
frame обязательна. Это narrow regression fix, не завершённая transactional IR.

Первый foundation hunk затем должен совместно связать:
- existing Renderer/headless/embedder callers и consuming PreparedSubmission;
- immutable tables/viewport/effect bindings и buffer cursor per encoded use;
- каждый production early submit, его charged resources и completion handoff;
- candidate retained target + reservation/copied partial baseline;
- authoritative first errors и discard только unsubmitted obligations.

Не вводить dead public submit wrapper, который лишь перенаправляет queue.submit
и не владеет buffers/charges/epoch/target. Private API сначала имеет реальные
production callers. Не объединять existing submits до freeze/reuse audit.

## Required discriminating tests после edits

- Два public flush разных геометрий в одном encoder/submit различают BufferPool
  overwrite; два target viewport sizes различают shared viewport overwrite.
- Два blur sizes/radii до общей submission сравниваются с независимыми GPU outputs.
- ShaderMask child replay failure: mask/copy не proceeds, first error delivered,
  next valid frame correct.
- После early clear/child/effect submit поздний failure сохраняет committed
  whole-target pixels и submitted quota до completion.
- Producer commands → mask/blur/copy/composite сохраняют ordering, а two CPU
  texture writes перед submit имеют documented latest texels semantics.
- Completion delayed и generation teardown: reservation releases exactly once,
  next small valid frame admits; CPU finish alone не completion.

Ни один из новых failure scenarios ещё не исполнен этим inventory. Evidence
текущих GPU baseline tests и cube capture хранится отдельно; здесь только source
callsites и obligations, нужные для безопасной реализации.
