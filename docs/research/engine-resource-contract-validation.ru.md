# External GPU resource contract: проверки и измерения

Дата: 2026-10-01. Windows, NVIDIA GeForce RTX 3070 Ti, DX12, wgpu 30.0.1.
`CARGO_BUILD_JOBS=1`, `--locked`, штатная nextest-конфигурация. Ранние GPU проверки и benchmarks использовали
shared target. `check-changed` отклонил его как внешний относительно checkout;
финальные gates и GPU run выполняются в изолированном target worktree.
Baseline — origin/main
`a0ab8c0e465e097dffb6648cac0dc82d59b7501a`. Другие адаптеры и browser WebGPU
этими локальными результатами не проверены.

## Контракт и воспроизводимые дефекты

SourceScene сохраняет логический TextureId. Engine recording захватывает
allocation и descriptor; замена ID после recording не перенаправляет draw.
Texels не копируются: запись producer в ту же allocation остаётся видимой.
Регистрация проверяет фактические dimension/layers/samples/usage/format до
create_view. Update сохраняет размер, формат и policy; ошибочный update
оставляет предыдущую регистрацию пригодной для следующего draw.
Sampling/color enums допускают расширение без downstream exhaustive matches.
Admission использует исчерпывающий match по color: добавление режима потребует
явной политики форматов, а не неявного использования прежнего SDR path.

На baseline новые строки существующей painter readback family упали:
nearest дал `[183, 0, 72, 255]` вместо `[255, 0, 0, 255]`; записанный draw
allocation A после update получил синий B вместо красного A. Остальные строки
семьи продолжили выполняться. Baseline production sources были временно
восстановлены из HEAD, а рабочие изменения возвращены byte-for-byte в finally.

Новая реализация проверяет explicit nearest/linear и resource default после
update, straight/premultiplied/opaque alpha при opacity 1 и 0.5, отсутствие
неявной смены allocation, вложенную opacity, неподдерживаемые imports,
сохранение старого draw после отказа и следующий исправный кадр.
Конкурирующие ошибки проверяются в обоих порядках: quota/missing ID и
257 gradient stops/missing ID через изоляцию сегмента. Первый отказ остаётся
authoritative; target не меняется, следующий кадр выполняется.

Дополнительное ревью нашло alpha halo у Straight + Linear: для двух texels
opaque red / transparent black на 32px quad пиксель x=12 должен дать red 183,
а интерполяция straight RGB/alpha с последующим умножением даёт 132.
Исправление premultiply каждого из четырёх mip-zero texels до интерполяции
сохраняет coverage; скрытый RGB полностью прозрачного texel не просачивается.
Это правильная alpha-интерполяция в текущем encoded SDR пространстве,
а не переход к физически корректному linear-light blending. Парный readback
с premultiplied источником проверяет также opacity 0.5 (red 92).
Этот путь требует четырёх texture loads вместо аппаратного linear sample;
Nearest, Opaque и уже Premultiplied сохраняют аппаратный sampling.

Private lifetime probe наблюдает именно engine lease: pixel readback сам по
себе не доказывает completion ownership, поскольку wgpu удерживает backing.
Проверяются фактический completion, submit/registration panic отдельно и вместе,
quarantine до teardown, затем progress нового owner. Два submit одного кадра
проверяют повторное включение cached binding в новый completion-owned bundle.

Команды:

```powershell
$env:FLUI_REQUIRE_GPU='1'
cargo nextest run --locked -p flui-engine --features testing,gpu-profiler --no-fail-fast
cargo clippy --locked -p flui-engine --all-targets --features testing,gpu-profiler -- -D warnings
cargo run --locked --example embedded_gpu_scene -- --capture <absolute PNG path>
cargo xtask check-changed
cargo xtask checks
```

Полный GPU run после shader fix в изолированном target: 58 passed, 0 skipped,
82.639 s. Painter family содержит 24 rows, включая RGBA/BGRA alpha halo;
после исправления clippy в BGRA upload helper она повторно прошла за 23.209 s.
Clippy all-targets с testing/gpu-profiler прошёл с `-D warnings`.
Embedded GPU example capture прошёл fixed-angle assertions для depth, rotation,
foreground 2D overlay и progress на том же DX12 adapter; PNG визуально проверен.
Заключительные `check-changed` и `checks` прошли: workspace 608 passed,
10 skipped; strict rustdoc и doctests; cross-typecheck Win32/AppKit/Android/iOS,
Android composition root, desktop-mcp Win32/AppKit и wasm32.
iOS runner пропущен, поскольку требует macOS; Linux platform suite требует
xvfb-run и остаётся проверкой CI. WGSL gate подтверждает uniform control flow
для четырёх derivative-taking функций, включая изменённый texture shader.

## Mutation checks

Намеренное удаление completion lease приводит к отказу private lifetime probe
до poll. Игнорирование opaque metadata и отсутствие RGB opacity scale для
premultiplied источника ломают аналитические pixel assertions. Каждый запуск
возвращает nextest exit 100, source возвращается в finally. Удаление смены
pending bundle epoch также возвращает exit 100: падает repeated-submit lifetime
assertion о владении второго submit после CPU finish.
Две дополнительные мутации нового alpha path проверены в изолированном target:
отключение dispatch дало `[131, 0, 0, 255]` вместо red 183, удаление premultiply
texels дало `[183, 0, 72, 255]` вместо чистого красного. Оба запуска завершились
exit 100 именно на pixel assertions; source восстановлен byte-for-byte.

## CPU recording и encoding

Benchmark `render_throughput::external_bindings`: target 256x256, 32/256
команд, 1/16 allocations, источник 16x16, linear sampling, straight alpha.
Измеряются record и render_to_view; begin, encoder creation, submit,
completion drain и finish выполняются вне измеряемого интервала. GPU timestamps,
RSS/driver VRAM и p99 latency не измерены. Это центральная оценка Criterion,
а не межплатформенный frame budget или гарантия отсутствия всех регрессий.
Замеры ниже сделаны до исправления alpha halo и относятся к CPU bookkeeping;
они не оценивают GPU стоимость ручной четырёхточечной alpha-интерполяции.

Baseline: 10 samples, warm-up 1 s, measurement 2 s. Финальный run: 20 samples,
warm-up 2 s, measurement 4 s, profile dev в обоих случаях.

| Allocations / draws | Main | После | После: confidence interval |
|---|---:|---:|---:|
| 1 / 32 | 664.76 µs | 205.56 µs | 202.88–208.76 µs |
| 1 / 256 | 4.8125 ms | 1.4120 ms | 1.4021–1.4218 ms |
| 16 / 32 | 330.16 µs | 287.17 µs | 284.67–289.95 µs |
| 16 / 256 | 2.4086 ms | 1.4946 ms | 1.4777–1.5163 ms |

Первый короткий after run для 16/256 дал 3.85 ms и показал regression.
Повторный более длинный run до дополнительной оптимизации дал 1.48 ms;
поэтому величина этого первого скачка не приписывается изменению архитектуры.
Ревью независимо нашло лишние lease permits для round-robin draws.
Теперь cached binding включает lease и charge один раз на pending submit bundle,
а следующий submit повторно удерживает их; переполнение epoch отключает reuse
без wrapping. Final run не доказал отдельного ускорения именно этой оптимизации,
но она убирает зависимость числа этих bookkeeping allocations от числа draws.

```powershell
cargo bench --locked -p flui-engine --features testing --bench render_throughput --profile dev -- external_bindings --sample-size 20 --warm-up-time 2 --measurement-time 4 --baseline external_main
```

Для baseline используется тот же benchmark с прежней register signature,
production main и `--save-baseline external_main` вместо `--baseline`.

## Проверенные reference shapes и оставшиеся границы

После выбора дизайна проверены [wgpu Texture metadata](https://docs.rs/wgpu/30.0.1/wgpu/struct.Texture.html)
и [egui-wgpu texture bindings](https://github.com/emilk/egui/blob/main/crates/egui-wgpu/src/renderer.rs):
metadata доступна до создания view; reuse binding должен учитывать sampler
и использовать layout своего pipeline. FLUI сохраняет painter order и свой
engine-local lease protocol, а не переносит чужую архитектуру.
Alpha filtering дополнительно сверено с
[egui Color32](https://github.com/emilk/egui/blob/main/crates/ecolor/src/color32.rs):
premultiplication полезна для корректной интерполяции даже в nonlinear sRGBA.
FLUI выполняет её при sampling raw external texture, сохраняя возможность
producer обновлять ту же allocation без промежуточной GPU копии.

Этот delivery не предоставляет managed allocation factory, texel snapshots,
content revision/wake для quiescent SourceScene, whole-engine VRAM cap,
linear-light/HDR pipeline или closing diagnostic service. Raw texture provenance,
clone.destroy и конкурентные внешние submit остаются trusted boundary.
Prepared metadata charges — conservative requested bookkeeping model,
а не точное физическое потребление памяти. Следующая производственная поставка:
managed producer commit с content revision, dependency damage и realm wake;
затем общий allocation ledger и coherent working color contract.
