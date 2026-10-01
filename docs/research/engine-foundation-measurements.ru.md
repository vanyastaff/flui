# Engine foundation: измерения и границы доказательств

Дата: 2026-10-01. Windows, NVIDIA GeForce RTX 3070 Ti, DX12; один compiling
worker, `--locked`, `--profile dev`. Это локальная проверка одного адаптера.
Данные не устанавливают мобильный, браузерный или межплатформенный бюджет.

## До изменения Command IR

Criterion: 10 samples, warm-up 0.2 s, measurement 0.5 s,
`--save-baseline engine-foundation-before`. Значения ниже — центральная оценка
Criterion, не p99. GPU completion входит в wall-clock round trip; GPU timestamp
и отдельно CPU encode/prepare здесь не измерены.

| Сценарий | До |
|---|---:|
| 50 rects + gradient + text | 192.12 µs |
| Solid, 32 commands | 112.59 µs |
| Solid / linear gradient, 32 commands | 263.43 µs |
| Linear / radial gradient, 32 commands | 290.93 µs |
| Solid, 256 commands | 198.50 µs |
| Solid / linear gradient, 256 commands | 338.43 µs |
| Linear / radial gradient, 256 commands | 289.54 µs |
| Mask 256 × 256 | 154.78 µs |
| Blur 256 × 256, sigma 5 | 861.88 µs |

Blur имеет широкий confidence interval 583–1244 µs: короткий прогон не даёт
оснований объявлять небольшую разницу ускорением. Старый renderer ошибается
на alternating order: его время — стоимость старого поведения, а не доказательство
эквивалентного результата.

`damage_retained_target`: full 4/16/64 layers — 361.66 µs / 1.0904 ms /
3.4580 ms; partial + blit — 328.20 / 324.00 / 334.46 µs; blit — 294.21 µs.
Старый partial benchmark не создаёт transactional candidate и не исполняет
FrameProtocol; нельзя выдавать его за стоимость новой защиты committed image.
Один BGRA8 target 1920 × 1080 занимает 8,294,400 payload bytes; candidate и
committed вместе — 16,588,800 bytes без driver overhead и остальных ресурсов.

Команды для основных baseline:

```powershell
cargo bench --locked -p flui-engine --features testing --bench render_throughput --profile dev -- --sample-size 10 --warm-up-time 0.2 --measurement-time 0.5 --save-baseline engine-foundation-before
cargo bench --locked -p flui-engine --features testing --bench offscreen_resource_cache --profile dev -- --sample-size 10 --warm-up-time 0.2 --measurement-time 0.5 --save-baseline engine-foundation-before
```

Ordered completion baseline получен из сохранённого до миграции benchmark
binary с фильтром `cpu_observed_record_encode_submit_completion` и теми же
параметрами. Первоначальный record-only benchmark ошибочно копил незавершённые
команды и достиг лимита vertex buffer; его результат отброшен. Исправленный
`cpu_record_drained` измеряет recording, а drain выполняет вне измеряемого
интервала. Для него старого достоверного baseline нет.

Ignored локальные журналы: `target/engine-audit/baseline-render.log`,
`baseline-effects.log`, `baseline-ordered-complete.log`. Criterion хранит samples
в target directory. Эти локальные файлы не являются переносимым CI artifact.

## Проверка ошибок до исправления

`FLUI_REQUIRE_GPU=1` исключает успех через отсутствие GPU. В gradient family
до ordered IR воспроизведены три ошибки: gradient перед solid, разные stop
tables в opacity layer и порядок linear/radial/linear. Журнал
`target/engine-audit/ordered-ir-before.log` содержит фактические неверные pixels.
После первого изменения все три строки прошли; новая строка девятого stop
обнаружила оставшийся clamp в CPU construction. Это отдельный дефект, а не
основание уменьшить ожидаемое число stops.

Acceptance требует повторного GPU прогона, mutation проверки различающих
sample points, сравнения производительности после исправлений и review
реального кода. План и успешная компиляция этих доказательств не заменяют.

## После ordered replay и кеша состояния render pass

Те же параметры короткого Criterion run, baseline
`engine-foundation-state-cache`, журнал `after-state-cache.log`:

| Сценарий | До | После |
|---|---:|---:|
| 50 rects + gradient + text | 192.12 µs | 190.02 µs |
| Solid, 32 commands | 112.59 µs | 140.14 µs |
| Solid / linear, 32 commands | 263.43 µs | 271.98 µs |
| Linear / radial, 32 commands | 290.93 µs | 312.46 µs |
| Solid, 256 commands | 198.50 µs | 243.87 µs |
| Solid / linear, 256 commands | 338.43 µs | 677.36 µs |
| Linear / radial, 256 commands | 289.54 µs | 509.88 µs |

Это не общее ускорение. Основная смешанная сцена находится в прежнем диапазоне,
но bounded recording и корректный painter order имеют стоимость. До кеширования
состояния linear/radial 256 занимал 1.0384 ms; устранение повторных bindings и
scissor заметно сократило её. Старый неправильный порядок объединял разные
позиции в несколько batch draws; новый сохраняет переключения между ними.
Следующий кандидат на оптимизацию — единый gradient instance/shader для linear,
radial и sweep с сохранением painter order, затем совместимое представление
solid/gradient. Требуются собственные pixel/mutation проверки; повторная сортировка
по категории не является допустимой оптимизацией.

У candidate cost model и legacy partial benchmark различались точки ожидания:
legacy ждёт дважды. Добавлен `partial_queued_blit_128px` с одним конечным ожиданием,
совпадающим с candidate model. При четырёх layers он показал 186.25 µs, candidate
allocation + copy — 295.01 µs. При 16 layers — 245.30 и 298.40 µs. При 64 layers
результаты инвертировались и имели широкие intervals: 426.27 и 304.44 µs. Эти
короткие samples нельзя использовать для вывода, что копирование ускоряет кадр.
Модель не исполняет FrameProtocol/permit ledger; её назначение — показать
allocation/copy cost с одинаковой схемой ожидания. Мобильная bandwidth и p99
остаются неизмеренными.

Повтор с 20 samples, warm-up 1 s и measurement 2 s для четырёх layers
(`after-retained-reuse.log`, baseline `engine-foundation-retained-reuse`):

| С одинаковым конечным ожиданием GPU | Оценка | Confidence interval |
|---|---:|---:|
| Partial + blit, без candidate | 193.59 µs | 188.90–198.30 µs |
| Новая candidate texture + copy + partial + blit | 278.61 µs | 264.55–301.05 µs |
| Переиспользуемая candidate + copy + partial + blit | 221.11 µs | 211.37–230.91 µs |

Production RetainedTarget теперь меняет местами committed и совместимый spare.
Это устраняет allocation каждый кадр, но сохраняет дополнительную текстуру и
копирование перед partial repaint. В этой cost model цена защиты после reuse —
около 27.5 µs относительно matched control. При отказе candidate освобождается,
committed сохраняется; resize и device-domain mismatch запрещают reuse. Проверка
под квотой ровно на две texture включает несколько успешных кадров, отказ,
следующий успешный кадр и resize с чтением пикселей.

Effects после изменений (`after-effects.log`, короткий run): mask — 129.63 µs,
blur — 270.49 µs. Старый blur baseline шумный и имел ошибку uniform slot; результат
не доказывает общее трёхкратное ускорение корректного blur.

## Проверка чувствительности тестов

После зелёного прогона 58/58, 0 skipped (`ir-final-readbacks.log`) временно
возвращены три старых дефекта. `ir-mutations.log` фиксирует реальные отказы:
`ninth_stop` получает красный вместо синего; `cached image scissor changes`
теряет левое изображение; `frozen viewport between flushes` теряет первый draw
из-за повторного использования буфера до submission. Исходники восстановлены
в `finally`. Три ошибки painter order отдельно воспроизводились до миграции.

## Финальная проверка перед PR

`foundation-gpu-final.log`: 58/58 GPU tests, 0 skipped после state caching и
двухслотового target reuse. `foundation-check-changed.log`: полный
`cargo xtask check-changed` завершился с exit 0, включая 608/608 общих тестов
(10 skipped), clippy, strict docs/doctests, Android app и wasm typechecks.
Linux/xvfb platform suite и iOS app runner на Windows не исполнялись.

Для Android C assembly в окружении команды заданы PATH с LLVM bin и
`AR_aarch64-linux-android=C:/Program Files/LLVM/bin/llvm-ar.exe`;
xtask задаёт clang и target flags. Более специфичная переменная cc-rs выбирает
установленный llvm-ar вместо отсутствующего Unix ar. Это настройка инструментов
хоста, не ослабление проверок. Independent source review ownership/retirement
после всех исправлений не нашёл новых блокирующих дефектов; поздние driver faults
и native mobile/WebGPU выполнение этим не подтверждены.

Финальный запуск embedded_gpu_scene --capture завершился с exit 0: analytical readback подтвердил depth, rotation, 2D overlay и progress. PNG foundation-gpu-scene.png визуально проверен; журнал foundation-gpu-scene.log в target/engine-audit. Это проверка конкретной сцены на DX12, не общий 3D API contract.
