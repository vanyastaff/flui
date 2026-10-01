# Фундамент FLUI до 1.0: решения, которые нельзя откладывать

Дата аудита: 1 октября 2026 года. Это конкретное предложение миграции,
а не принятый ADR. Статусы ниже получены чтением текущих ADR, ARCHITECTURE
и source; сборки и новые acceptance tests здесь не выполнялись. Изменения
cross-crate contracts должны пройти отдельные ADR до реализации.

## Решение

До 1.0 нужно закрепить ownership, единицы координат, meaning цвета и alpha,
идентичность/время жизни ресурсов, ordered effects и их read/write regions,
frame outcomes и расширяемость capabilities/SDK. Реализацию HDR output,
remote transport и ускоренных compute paths можно выполнять позже **только
если эти формы уже допускают её без изменения смысла существующего UI**.

Необходимость формы доказывается текущими препятствиями: `Color` теряет
wide-gamut значения при хранении `u8`; один clip slot не представляет
пересечение двух сложных clips; TextureId сам не удерживает ресурс;
LifecycleContext закрывает third-party platform services; damage/lowering
могут расходиться на extent эффектов. Это задачи архитектуры сейчас.

## Что уже есть и что необходимо закрепить

| Область | Уже реализовано / источник | Недостающая форма до 1.0 | Цена пропуска |
|---|---|---|---|
| Ownership | `UiRealm: !Send + !Sync`, `UiRealm::pump`, typed bounded ingress; runtime без host/GPU graph; SurfaceLease [A–C] | Завершить closed owner-operation admission и extraction, сохранив close fences, bounded continuation и unwind obligations; единый lifecycle ресурсов presentation/device | Последующее multiwindow/embedding требует пересобрать routing, callbacks и state ownership |
| Координаты | FLUI-owned logical `f64`, device aliases, DPR, compile-fail; snap vs cover [D] | Одна transform/rebase vocabulary для clips, effects, damage и follower; policy snapping/animation explicit; ограничение affine/projective выражено результатом | Любой cropped effect, monitor DPR change или perspective порождает независимые исправления во всех carriers |
| Цвет/alpha | `Color` — straight-alpha sRGB u8; premultiplied lerp; encoded blending и explicit UNorm+sRGB presentation [D,E] | Float color representation с explicit source/working spaces, gradient interpolation/alpha policy; одинаковые alpha contracts images/offscreen/filters | HDR и wide gamut требуют заменить публичный Color, shaders, cache keys и semantics gradients после стабилизации |
| Ресурсы | `ShapedParagraph` удерживает FontBlob; engine-owned registries/pools, RAII intermediates [F,E] | Для image/external texture согласовать asset identity, content revision, device allocation и lease для recorded scenes; descriptor alpha/color/format/dimensions | Retained/remote replay может видеть исчезнувший/заменённый ресурс; recovery требует ad hoc повторных регистраций |
| Effects/clip | Closed Layer/DrawOp, ordered FilterOp items в этой рабочей ветке; damage boundary stamps; ADR-0099 region semantics [G,H] | Clip expression хранит все intersections/differences; effect input/output/dependency regions и composite semantics имеют один authoritative расчёт | Слои теряют clip, extent и siblings; partial repaint нарушает картинку; каждый новый эффект дублирует ошибки |
| Outcomes | `PresentDisposition`, sink submit verdict, damage debt/retained target [E,I] | Resource/budget/not-ready outcome совместим с существующей debt/lifecycle machine; advertised guarantee present vs display однозначна | Work исчезает при отказе, pending waits висят, asynchronous host расширяет неоднозначный результат |
| Capabilities/SDK | Backend-free contract crate, LifecycleContext sealing, measured SDK reexports, train guard, existing agent wire [B,J,K] | Принять/исправить ADR-0084 open typed provider seam, instance scope и override policy; определить version ownership/unknown capability поведение и доказать packaged external extension | Third-party service требует правки flui-view; packages зависят от внутренней topology или второго набора типов |

«Реализовано» не означает, что весь ADR принят: ADR-0081/0082/0083/0088/0098
имеют частичные/Proposed части. ADR-0087 принял damage/retained §3–§4,
но shared lowering, raster contract move и CPU §1–§2 остаются Proposed.
ADR-0099 Proposed и перечисляет известные ограничения implementation.
Эти статусы нельзя закрыть ссылкой на общий green gate.

## Каноническая форма

### Владение и frame boundary

Сохранить имеющуюся цепочку: host владеет native presentation и GPU device;
realm владеет UI/text/focus transaction; SceneSnapshot передаёт immutable
результат. Ни shared mutable widget tree, ни render-node `async` не нужны.
Каждая worker result привязана к существующему presentation address и
generation owner; после close/recovery stale completion не применяется.

Следующий конкретный шаг — завершить ADR-0083 typed owner-operation seam,
а не создать ещё один runtime. Исход операции обязан либо сохранять
обязательство с future wake, либо завершить его на closed/superseded owner.
Callback не допускает повторный вход в активную transaction своего owner;
cross-owner dispatch проходит typed admission и close/generation checks.
Он не получает прямого доступа к mutable state другого realm.

### Геометрия и clipping

Public geometry остаётся logical `f64`; GPU narrowing — один declared
boundary. Crop target имеет integer device origin/dimensions, а mapping
device→target применяется ко всем позициям, scissors, clips и texture UV.
Существующие `cover`/`snap_edges` — единственный rounding источник.

Вместо текущего одного `ResolvedClip` slot в lowering требуется immutable
clip chain/expression: intersect и difference с exact local shape+transform,
отдельно conservative device bounds. Простые rects схлопываются в scissor,
один rounded clip — SDF fast path; остаток идёт в exact mask/stencil
реализацию. В engine IR можно хранить frame-local clip identity и mapping;
новый публичный Layer plugin интерфейс для этого не требуется.

Save-layer region и ambient clip независимы: rotated quad не заменяется
AABB, когда ambient rounded clip занимает shader slot. Coverage применяется
в правильной точке группы один раз; destination-replacing modes сохраняют
часть destination вне coverage. Fast reintegration допустим только при
сохранении bounded region ADR-0099, а не просто при opacity=1.

### Цвет и alpha

Принять/скорректировать отложенную §7 ADR-0098 **до** заморозки Color:
FLUI-owned float components с определённой source space; именованные
8-bit constructors сохраняют удобство без хранения всего в u8.
Задать handling negative/out-of-range/nonfinite значений и alpha, equality
и cache hashing через canonical float policy. Existing linear interpolation
публичного Color и interpolation gradient — разные контракты; градиент явно
задаёт interpolation space и alpha space, включая defaults.

Согласовать paint/image/glyph/mask/filter outputs: straight input преобразуется
в premultiplied working representation перед blend; image metadata отличает
premul от straight и encoded от linear. Offscreen descriptor и pipeline key
включают working representation, чтобы format cache не смешал значения.
Encoded-sRGB compatibility policy сохраняется явно до перехода, а не через
неявную двойную conversion. HDR surface/tone mapping реализуются позже;
публичный Color и descriptors уже должны удерживать их исходные значения.

Минимальный contract комплект должен пройти существующий production draw,
а не остаться новыми неиспользуемыми hooks:

| Граница | Контракт сейчас | Production consumer |
|---|---|---|
| Paint Color | Components + source primaries/transfer; alpha отдельно; определённая range policy | Recorder и batch lowering преобразуют input в выбранное working space |
| Gradient | Interpolation color space + straight/premultiplied alpha policy; stop values сохраняют range | Три существующих gradient shaders используют одинаковую подготовку/interpolation |
| Image/external texture | Storage format, primaries/transfer, straight/premultiplied alpha, dimensions и revision | Upload/registry validation и texture sampling преобразуют sampled значения согласованно |
| Offscreen/filter | Working transfer/primaries + alpha representation + format | Pool key, blur/color-matrix/mask и composite pipelines выбирают совместимые targets |
| Blend | Операция выполняется в объявленном working space на premultiplied inputs | Shapes, gradients, images, glyphs и offscreen composites подчиняются одной policy |
| Presentation/readback | Working→output conversion ровно один раз; выходной format и display space согласованы | Текущие surface selector, final blit и capture/readback |

Текущий working policy — encoded sRGB с существующим blending; сохранение
этого поведения является SDR compatibility fallback. Выбор linear working
space вводится как явная policy с end-to-end tests, не локальным флагом одного
shader. Для unsupported wide-gamut/HDR output преобразовать в определённый
SDR sRGB результат: gamut mapping/clipping и tone mapping policy заданы
заранее, предсказуемы и не зависят от случайного UNorm saturation.
Реализация может начинаться с только SDR output, но уже должна корректно
преобразовывать declared источники; нельзя обещать HDR retaining float
values и молча отправлять их в encoded UNorm target.

Acceptance различает float storage и color management: одна физическая
краска, заданная эквивалентно в разных source spaces, совпадает после SDR
conversion; linear-vs-encoded translucent blend даёт заранее рассчитанные
разные swatches; intermediate roundtrip не делает повторный encode или
premultiply; transparent source RGB не создаёт halo; external texture с
каждым допустимым alpha/transfer descriptor совпадает с CPU oracle.

### Image/texture lifetime и bounded execution

Не переносить GPU handles в Scene. FontBlob ownership в ShapedParagraph —
существующий образец completeness. Для обычного image recorded command
удерживает immutable asset payload или lease его resolver; для external
dynamic texture policy фиксирует late-bound volatility и content revision.
Asset identity не равна painter-local allocation identity; device generation
инвалидирует вторую, но не уничтожает первую.

Прежде изменить `ExternalTextureRegistry::register/update/unregister`,
зафиксировать: какие retained commands видят обновление; что означает
freeze; когда unregister делает asset unavailable; как уже принятый frame
удерживает allocation до submission; кто восстанавливает resource при
device recovery. Descriptor содержит проверенные texture dimensions,
sample/usage/format и alpha/color metadata. Неподходящая texture даёт error
до GPU validation panic, а не warning и некорректный draw.

Один внутренний frame resource/cost plan, рядом с нынешним Command IR,
считает peak intermediate bytes, passes, upload bytes и work counts.
Scene preflight дополняется reservation перед каждой дорогой preparation,
upload и allocation: сейчас запись уже загружает images, rasterizes glyphs
и выделяет offscreens, поэтому проверка лишь до submission опоздает.
Он управляет lifetime фактов, а не дублирует tree. Лимиты
приходят от host/device policy. Переполнение арифметики и превышение бюджета
дают structured outcome; не показывать частично выполненный frame и не
деградировать clip/filter молча. Eviction не удаляет resource ещё записанного
draw; decode/compile pending work bounded и wakes сохраняют debt.
Asset ingress ограничивает compressed и decoded bytes, dimensions и стоимость
одной задачи, отдельно от длины очереди. Parser/rasterizer/tessellation
получают checked inputs и cancellation/work limits; strict safety limits
отличаются от soft quality budgets. Если библиотека не гарантирует work limit,
контракт явно задаёт containment и пределы входа: cancellation сама по себе
не прерывает произвольную синхронную библиотеку.

### Effect dependencies и damage

У каждого эффекта зафиксировать ordered input, output region, required
input region, backdrop-read region, mapping и composite mode/coverage.
Это shared facts между `flui-layer::LayerDiffer` и engine lowering:
невозможно независимо вычислить blur reach и объявить их одинаковыми.
Поддержать current closed effect enum exhaustive matching; добавление нового
variant требует dependency semantics и backend lowering в одной migration.

Общий semantic scene visitor должен обслуживать window/capture сейчас;
current headless approximations mask/backdrop/follower не допускают достоверных
goldens. Решить ownership этого lowering в рамках Proposed ADR-0087: если
перенос в flui-layer принят, туда идут GPU-free facts/discipline, Command IR
остаётся engine; если отвергнут — явная revision ADR, общий visitor внутри
engine. CPU raster implementation можно отложить после этого решения.

### Capability и package extensibility

ADR-0084 уже содержит конкретный object-safe erased acquisition плюс typed
extension и provider registration; это предпочтительнее закрытого роста
LifecycleContext. Framework capabilities остаются sealed lifecycle methods,
platform set открывается через contract crate, registries per scope.
Application overrides и deterministic duplicate-provider errors обязательны;
нет link-time globals или provider registration в build.

Rust in-process TypeId идентичность остаётся внутри одной dependency train;
stable capability name — для diagnostics, не proof совместимости и не
runtime ABI. Semver принадлежит interface crate/SDK согласно ADR-0088;
не изобретать параллельный числовой version для каждого локального trait.
На serialized boundary unknown tools/optional fields и negotiation следуют
существующему ADR-0080/0095; новый remote transport согласует version/limits
там, а не сериализует Rust trait object или Scene автоматически.

Host-free SDK переэкспортирует те же типы. Перед1.0 нужны package tarball
parity и внешний fixture: custom widget, theme extension и custom platform
provider через SDK+contract crates, включая app override и два windows.
Не добавлять hypothetical GPU hooks: сначала выдать package автору
существующий engine-free путь расширения и завершить plugin half ADR-0088.

## Порядок миграции и acceptance

| Последовательность | Изменение | Обязательная проверка |
|---|---|---|
| 1 | Принять границы ownership/lowering/capability ADR; статусы синхронизировать с source | Runtime/product/harness используют одну pump; two realms close/reenter/unwind с последующим progress; compile-fail запрещает capability в build |
| 2 | Canonical color/alpha/gradient descriptors и image resource semantics | Цвет вне sRGB/8-bit не теряется до lowering; known sRGB swatches; transparent white mask identity; градиент с выбранным space; alpha=0 и nonfinite contracts |
| 3 | Shared exact clip/effect region facts и visitor window/capture | Nested rrect+path+difference, rotated bounded layer, AA Clear/Src fringe; mask modes SrcIn/Modulate; follower transform; capture совпадает с production content path |
| 4 | Resource lease/revision/device generation и cost preflight | Записать image, evict/unregister/update до replay; frozen vs volatile; wrong format/usage/size; recovery; too-large nested effect возвращает отказ и следующий valid frame работает |
| 5 | Damage читает shared dependency regions | Full-vs-partial sequences: moving nested blur, changed backdrop, removed translucent bounded layer, DPR change, failed present и superseded frames |
| 6 | Open provider seam и SDK package parity | External tarball custom widget/provider без internal crates; duplicate order deterministic; missing optional vs required; override; два окна; release agent endpoint inert |
| 7 | Performance implementation на закреплённой форме | Flat/nested crop pixel equivalence; first-use pipelines; peak VRAM и p95/p99; бюджеты проверены на desktop/mobile/web capability sets |

Шаги 2–6 можно вести disjoint частями после согласования контрактов шага1;
cross-crate invariant принимается целиком, без фазы, где новый API публичен,
а production path читает старую форму. Acceptance tests должны становиться
красными при revert production hunk, а не дублировать expected predicate.

## Что действительно можно реализовать позже

### Проверка расширяемости до стабилизации

Игровой/3D renderer и AI runtime — обязательные сценарии проверки формы,
даже если их полные реализации не входят в текущий релиз. Для GPU content
нужно решить device/queue ownership, texture lease и producer completion,
format/color/alpha, resize/device-loss, clipping/compositing и отсутствие
ожидания producer на UI frame path. Отдельный swapchain не должен обходить
z-order, capture или lifecycle окна. Прототип embedded animated texture
должен пережить resize, removal и recovery, а capture — показать тот же кадр.

Game clock может требовать fixed-step simulation, а presentation — variable
cadence: scheduler согласует wake и cancellation, не заставляя idle UI
непрерывно рисоваться. Input routing/focus и semantics имеют явное владение;
игровая поверхность не должна перехватывать события соседнего widget.

AI runtime получает scoped service через capability/SDK boundary и доставляет
bounded results в owner ingress. Проверить streaming, superseded request,
закрытие realm, cancellation, budget и host authorization tools; provider
не читает/меняет render tree напрямую. Это отдельный runtime integration,
а не запрет AI в приложении и не inference внутри paint.

- HDR output после float/color/alpha contracts: surface negotiation,
  tone mapping и target formats — реализации уже выразимой семантики.
- Remote transport после resource identity/lifetime и existing protocol
  scopes/version/errors: transport остаётся отдельным; scene wire export
  требует отдельного format decision, не обязателен для video+semantics.
- Compute rasterization на wgpu после exact clip/effect semantics и
  capability/cost negotiation, с прежним видимым результатом.
- Cache/AA/backend performance strategies после conformance suite.

Нельзя откладывать форму Color, exact effect/clip dependencies, leases,
closed lifecycle outcomes и third-party capability acquisition, ссылаясь
на отсутствие конечного HDR/remote feature. Именно эти формы определяют,
понадобится ли потом переписывать ядро.

## Репозиторные основания

- **A:** [flui-runtime Architecture](../../crates/flui-runtime/ARCHITECTURE.md), [ADR-0083](../adr/ADR-0083-one-frame-transaction-in-flui-runtime.md).
- **B:** [flui-platform-api Architecture](../../crates/flui-platform-api/ARCHITECTURE.md), [ADR-0082](../adr/ADR-0082-platform-api-contract-crate.md).
- **C:** [ADR-0081](../adr/ADR-0081-workspace-tiers-and-reach-facts.md), [ADR-0027](../adr/ADR-0027-owner-affine-ui-realms.md).
- **D:** [ADR-0098](../adr/ADR-0098-owned-f64-geometry-values.md), Implementation status.
- **E:** [flui-engine Architecture](../../crates/flui-engine/ARCHITECTURE.md), record/replay, mapping decisions и Open items.
- **F:** [flui-painting Architecture](../../crates/flui-painting/ARCHITECTURE.md), ShapedParagraph; [ADR-0092](../adr/ADR-0092-per-realm-text-over-parley.md).
- **G:** [flui-layer Architecture](../../crates/flui-layer/ARCHITECTURE.md), boundary stamps и effect footprints; [ADR-0087](../adr/ADR-0087-raster-contract-and-cpu-backend.md).
- **H:** [ADR-0099](../adr/ADR-0099-save-layer-region-and-blend.md).
- **I:** [raster outcomes](../../crates/flui-engine/src/raster.rs), [external texture registry](../../crates/flui-engine/src/external_texture_registry.rs), [Command IR](../../crates/flui-engine/src/command_ir.rs).
- **J:** [flui-sdk Architecture](../../crates/flui-sdk/ARCHITECTURE.md), [ADR-0088](../adr/ADR-0088-official-packages-sdk-and-facade.md), [ADR-0084](../adr/ADR-0084-open-capability-seam-and-plugins.md).
- **K:** [ADR-0095](../adr/ADR-0095-agent-protocol-schema-crate.md), authenticated local endpoint и window-scoped handles.

Ограничение: `crates/flui-assets/ARCHITECTURE.md` не найден; ресурсные
выводы опираются на painting/engine source и приведённые ownership records.
Новая работа в этой записке — проверка форм и предложение migration;
перечисленные acceptance tests ещё нужно реализовать/запустить.
