# Animation: выпускные свидетельства

Дата проверки: 2026-10-12. Production revision:
`61ea713380a449777ffa791b61a3a01e3fd81c5a`.
Native assertion revision: `a73d351ce3ae9a6a0df3119d95304b0b80f09426`.
Validation code revision: `e0961a73f8650e6e5e497a23336c3cefbf6df509`.
Framework production-код относительно указанной базы не менялся; изменены
assertions, benchmark validation и foreground helper инструмента проверки.
A8 завершён в указанных границах Windows/native/GPU и измеренных workloads.
A9 остаётся открыт до зелёного CI финальной validation revision.
CI production-базы, локальный gate и CI каждой validation revision — отдельные receipts.

## Среда и метод

Windows x86_64 MSVC, Intel Core i9-13900K (24 cores, 32 logical processors),
Rust 1.99.0 (`b940084d7`), Balanced power scheme. Установлены Intel UHD Graphics 770
(driver 32.0.101.7088) и NVIDIA RTX 3070 Ti (driver 32.0.15.6094).
Перечень установленных GPU сам по себе не доказывает выбор адаптера тестом.

Шесть build jobs, incremental выключен, отдельный target каждого checkout.
Тяжёлые проверки и замеры используют один host lock. Частота CPU и внешняя
активность других приложений не контролируются; результаты не являются обещанием
постоянного speedup или GPU latency budget.

Registry baseline: неизменённый production `c297b6c89bc93941e28c738c9dfac3d623a2dae2`
с текущим `benches/vsync_registry.rs` (Git blob
`042f44b12717c542b74489f04f62aa374bf98f7f`). Snapshot с единственным изменением
этого fixture — `87e6eaa56ad2ef375b09e14eb90c42c861f56d52`.
Это исторический owner-local baseline, а не измерение полной миграции с прежней
process-global модели.

Mounted baseline: `6af2cf426809dbf8a33bbb415e904f0bfdc847c4`. Benchmark
`mounted_motion.rs` и support-файл побайтово совпадают с candidate (Git blobs
`a89c5b6d9b0e7854efc20ce0cc3d14aa0c398680` и
`55d8ceacf897321fa40c93ec1cee6b513441a4e1`). Сравнение оценивает изменения после
этого интегрированного пути, а не стоимость перехода с u8 Color.

## Измерения

Последовательные optimized registry-прогоны обеих сторон выполнены: 30 samples,
500 ms warm-up, 2 s requested measurement; обе команды завершились с exit 0.
Criterion увеличивает окно, когда выбранное число samples требует больше времени.
Median estimates в µs:

| Workload | Baseline c297b6c89 | Current |
|---|---:|---:|
| stopped / 100 | 1.501 | 1.625 |
| stopped / 1000 | 29.967 | 34.267 |
| stopped / 5000 | 225.606 | 265.188 |
| stopped / 10000 | 455.125 | 483.443 |
| running / 100 | 6.745 | 7.270 |
| running / 1000 | 81.856 | 93.278 |
| running / 5000 | 520.772 | 564.001 |
| running / 10000 | 1079.652 | 1133.298 |
| mixed / 1000 | 32.950 | 35.712 |
| nested / 100 / depth 1 | 9.273 | 7.922 |
| nested / 100 / depth 8 | 6.386 | 7.651 |
| nested / 100 / depth 32 | 7.603 | 12.401 |
| nested / 1000 / depth 1 | 81.583 | 93.130 |
| nested / 1000 / depth 8 | 78.703 | 90.089 |
| nested / 1000 / depth 32 | 82.558 | 89.923 |
| unregister / 1000 | 106.017 | 127.252 |
| unregister / 10000 | 1372.595 | 1900.454 |

95% bootstrap median intervals for running/10000: baseline 1055.167–1108.397 µs,
current 1085.653–1165.995 µs; stopped/10000: 446.830–472.190 versus
474.968–511.050 µs; unregister/10000: 1337.257–1500.046 versus
1743.927–2021.729 µs. Эти результаты не обещают uniform speedup; более свежий
integration control нужен для атрибуции различий после исторического baseline.

Повторный registry control сравнил integration baseline `6af2cf4` и current:
50 samples, 1 s warm-up, 3 s measurement, exit 0 на обеих сторонах.
Median µs: running/10000 1088.156 → 1171.278; stopped/10000 445.343 → 466.512;
nested/100/depth32 7.619 → 8.292; nested/1000/depth32 82.766 → 90.619;
unregister/1000 112.189 → 118.406; unregister/10000 1410.160 → 1402.850.
Первое значение unregister/10000 не повторилось. Эти последовательные замеры
на общем хосте не доказывают причинную регрессию или uniform speedup.

### Mounted frames

Обе стороны: `cargo bench --locked -p flui-widgets --bench mounted_motion --
--sample-size 30 --measurement-time 2 --warm-up-time 0.5 --noplot`, exit 0.
Все 32 workload rows измерены. Median µs; в каждой ячейке baseline → current:

| Workload / population | Active interior frame | Idle pump | Retarget + admission |
|---|---:|---:|---:|
| Color / 1 | 7.361 → 7.582 | 1.279 → 1.596 | 11.369 → 11.701 |
| Color / 100 | 272.676 → 274.717 | 3.180 → 3.593 | 769.305 → 761.704 |
| Color / 1000 | 3306.106 → 2782.889 | 53.355 → 33.764 | 12793.425 → 8479.650 |
| Opacity / 1 | 6.769 → 5.508 | 1.191 → 1.577 | 11.620 → 11.398 |
| Opacity / 100 | 124.332 → 118.983 | 3.345 → 3.650 | 744.259 → 719.865 |
| Opacity / 1000 | 1225.420 → 1147.220 | 35.117 → 33.515 | 7913.400 → 7845.338 |
| ColorAndOpacity / 1 | 6.857 → 7.292 | 1.152 → 1.591 | 14.981 → 15.371 |
| ColorAndOpacity / 100 | 315.613 → 315.092 | 5.500 → 5.884 | 1083.347 → 1080.441 |
| ColorAndOpacity / 1000 | 3385.454 → 3385.070 | 82.046 → 74.875 | 13715.750 → 14162.725 |

Scroll median µs: active 13.800 → 14.020; cancelled idle 1.132 → 1.573;
cancel admission 1.562 → 1.985; animate admission 13.284 → 13.863.
Fractional-alpha correctness control: 266.652 → 267.049 µs.
Untimed controls verify 96 progressing submitted scenes, population and semantics,
then eight idle pumps with zero build/layout/submission. Opacity has zero builds
during active frames; Color and ColorAndOpacity rebuild their property consumers.
Fractional alpha is a separate correctness workload, not an extra throughput claim.

### Animation primitives

`cargo bench --locked -p flui-animation --bench animation_bench` used the same
30/2 s/0.5 s settings, exit 0 on both sides. The historical fixture only migrated
FloatTween/ColorTween names to Tween; production remained unchanged. Fixture
SHA-256: `C4942E0748263E8ED5898B37001DBABE15F286135D16CFFCFEAE90E178005B56`.
Median ns, baseline → current:

| Workload | Median ns |
|---|---:|
| controller / forward | 141.599 → 137.086 |
| controller / curved_value | 14.025 → 13.904 |
| tick / linear | 45.301 → 48.351 |
| tick / ease_in_out | 60.830 → 63.151 |
| tick / 1 value + 1 status listener | 60.268 → 66.823 |
| tick / 4 value + 1 status listener | 100.499 → 103.844 |
| tick / friction | 56.756 → 59.967 |
| tick / spring | 66.186 → 69.799 |
| status fan-out / 1 | 207.875 → 196.969 |
| status fan-out / 4 | 249.095 → 236.028 |
| status fan-out / 8 | 338.337 → 332.733 |
| curve / linear | 0.393 → 0.390 |
| curve / ease_in_out | 13.096 → 13.107 |
| curve / elastic_out | 9.095 → 8.908 |
| curve / three_point_cubic_emphasized | 20.292 → 20.274 |
| tween / f64 | 0.813 → 0.794 |
| tween / offset | 4.044 → 4.025 |
| tween / color | 118.133 → 117.334 |
| spring / construction | 134.134 → 134.983 |
| spring / x + dx | 23.951 → 24.691 |
| spring / animated color frame | 338.942 → 346.749 |

После сбора timings усилен untimed oracle: `is_animating` до/после измерения,
progress на следующем frame и interior value у curved getter. Timed loops
не менялись. Новый byte-identical fixture обеих сторон имеет SHA-256
`771980A924AC891E81F31FC5E9216522666588D7C484EBCA411CB65CC1993B25`.
`cargo bench --locked -p flui-animation --bench animation_bench -- --test`
прошёл все 21 rows на каждой стороне; это smoke validation, не новые timings.
Negative control через public `set_value(0.5)` останавливает linear run,
сохраняя directional phase. Старый oracle завершился успешно; новый завершился
ошибкой `the linear run must be installed`. Fault удалён перед positive controls.

Дополнительный negative control подставляет `NaN` вместо результата следующего
кадра в untimed assertion действительного linear benchmark. Старое `assert_ne!`
принимает его (exit 0); проверка конечности обоих значений и их различия отвергает
`NaN` и `INFINITY` (exit 101, `must advance with finite values on the next frame`).
Это проверка benchmark oracle, не заявление о дефекте controller producer.
Подстановки удалены; все 21 штатные smoke rows проходят с новым oracle (exit 0).
Timed loops не менялись; этот smoke run не выдаётся за новые paired timings.

Повторный полный timed run использовал усиленный fixture `771980A9…` на обеих
сторонах: те же 30 samples / 2 s / 0.5 s, exit 0. Median ns:

| Workload | Baseline | Current |
|---|---:|---:|
| controller / forward | 232.887 | 160.297 |
| controller / curved_value | 24.712 | 14.847 |
| tick / linear | 78.196 | 51.719 |
| tick / ease_in_out | 96.427 | 67.160 |
| tick / 1 value + 1 status listener | 100.087 | 67.730 |
| tick / 4 value + 1 status listener | 160.886 | 107.298 |
| tick / friction | 86.210 | 63.033 |
| tick / spring | 118.406 | 71.808 |
| status fan-out / 1 | 298.366 | 234.075 |
| status fan-out / 4 | 375.039 | 255.168 |
| status fan-out / 8 | 548.307 | 355.014 |
| curve / linear | 0.977 | 0.727 |
| curve / ease_in_out | 34.188 | 14.496 |
| curve / elastic_out | 46.749 | 19.841 |
| curve / three_point_cubic_emphasized | 40.256 | 21.876 |
| tween / f64 | 0.832 | 0.932 |
| tween / offset | 4.611 | 6.808 |
| tween / color | 123.336 | 193.560 |
| spring / construction | 170.674 | 299.080 |
| spring / x + dx | 57.905 | 26.603 |
| spring / animated color frame | 678.560 | 371.551 |

Разброс между прогонами существенен даже при неизменённых timed loops;
например current spring construction — 134.983 и 299.080 ns. Ни первый,
ни повторный прогон не выбирается как универсальный performance score.
Без изоляции внешней нагрузки и CPU frequency нельзя причинно приписывать
эти различия production-коду; публикуются оба результата и их условия.

### Allocation boundary

`cargo bench --locked -p flui-widgets --bench mounted_motion_allocations`
completed on both revisions, with its positive 4096-byte allocation control.
Counters are allocation/reallocation requests and requested bytes, not peak/live
memory. Current totals across 96 advancing frames:

| Workload | Requests | Requested bytes |
|---|---:|---:|
| Color / 1 | 7873 | 2798016 |
| Color / 100 | 332161 | 91633344 |
| Color / 1000 | 3270337 | 823816896 |
| Opacity / 1 | 5761 | 2643264 |
| Opacity / 100 | 122305 | 66579264 |
| Opacity / 1000 | 1160641 | 625035072 |
| ColorAndOpacity / 1 | 8449 | 2883648 |
| ColorAndOpacity / 100 | 381601 | 90617664 |
| ColorAndOpacity / 1000 | 3752737 | 865419072 |
| Scroll | 16801 | 5193600 |

FractionalAlpha matches Color at each population. All measured workloads have
exactly 768 more requests and 76800 more requested bytes than the integration
baseline across 96 frames: 8 requests/800 bytes per pump, independent of population.
Retarget/animate/cancel admission likewise adds 8 requests/800 bytes. Each idle8
total changed from 96 requests/3200 bytes to 160 requests/9600 bytes, while still
performing zero builds/layout/submissions. The runtime now collects presentation
sampling/recovery cohorts (`ui_runtime/sampling.rs`); the full pump allocation
bound includes this infrastructure. It is distinct from the zero-allocation
warmed animation tick contract and does not claim zero-allocation idle runtime.

## Native и GPU

`cargo xtask gpu-test` завершился с exit 0: engine 68/68 (260.222 s, один slow),
facade readback table 1/1 (15.127 s), без skips. Это реальное исполнение shader,
layer/filter/damage и fractional-alpha readbacks; type-check не заменял запуск.
`cargo xtask device windows-input` и `cargo xtask device windows-a11y` завершились
с exit 0: `INPUT=PASS`, `A11Y=PASS`. Первый input attempt безопасно завершился
`CANNOT_VERIFY` при отказе foreground. Tooling теперь допускает bounded 30 s
для ручного фокуса; повторный запуск действительно исполнил input, без обхода
foreground/hit checks. В успешном запуске дополнительный wait не понадобился.
Native окно использовало NVIDIA GeForce RTX 3070 Ti / DX12 по app log.
Input miss сохранил count 0, hit дал 1, Tab/Enter дал 2. UIA проверила Invoke,
expand/collapse с отказом повторного перехода, точные range values 2.375, 10,
4.625, отказ 11 без мутации, следующий здоровый action и scroll 160 → 0.
Это real window/input/semantics execution, а не native framebuffer readback.
`FLUI_REQUIRE_GPU=1` исключает успешный self-skip при недоступном adapter/device.
OS-настройки пользователя не меняются: чтение physical producer и сообщения
собственному receiver не объявляются проверкой реального переключения настройки ОС.
Android duration scale и macOS/iOS execution нельзя проверить на Windows;
cross-typecheck остаётся только компиляционным доказательством.

Native preference run на assertion revision: 3/3 выбранных tests прошли за
0.063 s; 33 остальных отфильтрованы, а не исполнены. `preferences_contract`
читает реальную Windows setting, проверяет successive host lifetimes,
wrong-thread refusal, создание native window после reads, owner notification
через свой HWND и teardown. `native_preference_read_recovery` и
`preferred_language_buffer_contract` исполняют private failure seams.
Negative control с временно инвертированным native mapping уронил
`preferences_contract`: `Some(false)` против независимого `Some(true)`.
Production bytes восстановлены (Git blob `aeab8cf28d5df728942cfe1f301e17769b70d908`);
повторный public contract прошёл за 0.030 s (1 executed, 9 filtered).
Это проверка чувствительности нового assertion, не найденный production defect.

Animation all-features run: 148/148 (86.417 s), без skips. Единственный slow —
`repeated_interruption_keeps_position_and_velocity_continuous` (85.478 s),
завершён успешно. `a_steady_state_frame_allocates_nothing` и
`retiring_idle_registry_owners_allocates_nothing` прошли: zero allocation calls
и requested bytes на ограниченном warmed animation/relay/idle-retirement пути
с живым allocator control. Это не zero-allocation обещание всего mounted frame.

## CI и scope

### Исторические 32 red-сценария

29 имён исходных случаев сохранены в текущих animation tests. Их старые red
receipts не заменяются новым чтением исходников; текущий all-feature запуск
проверяет зарегистрированные семейства. Три изменённых шва:

| Исторический случай | Текущий контракт |
|---|---|
| `nan_frame_time_is_skipped` | Frame admission принимает `Duration`, где NaN не представим; `finite_clock_time_is_anchored` проверяет правильную finite-time траекторию. Raw-f64 API не восстанавливается |
| `frame_scheduled_hook_may_read_the_controller` | `wake_failure_preserves_delivery_and_recovery`: requester читает committed `is_animating`/status, после wake failure healthy tail доставляется и следующий run завершается |
| `frame_scheduled_hook_may_query_vsync` | `wake_replacement_retires_captures_outside_borrows`: requester заменяет hook того же registry, а outgoing capture проверяет `has_running` вне borrow; recovery и следующая доставка сохранены |

`rg -n '#\[ignore' crates/flui-animation/tests` не находит отключённых контрактов.

### Локальный gate

Scope gate `check-changed --base 61ea713380a449777ffa791b61a3a01e3fd81c5a`
завершился с exit 0 на validation code revision. Scope: flui-animation,
flui-platform, xtask и 24 dependents. Результаты:

- fmt и strict clippy всех targets, включая engine/testing;
- nextest: 733/733 за 589.400 s, 19 configured skips, два slow завершены;
  repeated interruption — 145.573 s;
- отдельный platform compiler guard: 1/1 за 11.985 s, без skips;
- strict private rustdoc; doctests: 472 passed, 175 ignored, без failures;
- Windows source-check с максимальными optional features и wasm32 checks;
- 56 library feature cases и 56 tests/benches/examples feature cases.

Apple/Android source checks локально пропущены из-за отсутствующих SDK/tools.
Linux native platform suite требует xvfb-run и исполнена в production CI;
локальные Windows preferences/input/UIA receipts приведены выше.
Ignored doc blocks, configured skips и platform compilation не расширяют scope
на неисполненные сценарии.

Первый attempt остановился на fmt одного assertion; после formatting-only
исправления обычный Windows запуск упёрся в self-rebuild `xtask.exe`
(`Access is denied`). Gate выполнен тем же скомпилированным runner из копии
в target этого checkout, с обычным host lock. Source/target не разделялись
между checkout. SHA-256 оригинала и копии:
`A1FC1730513DC35F113EE4A7C97D6184FB4C7F1D83611ADDE22A90957C43A28E`.
Воспроизводимый обход блокировки для изменённого xtask на Windows:

```powershell
cargo build --locked -p xtask
Copy-Item -LiteralPath target/debug/xtask.exe -Destination target/xtask-check.exe
./target/xtask-check.exe check-changed --base 61ea713380a449777ffa791b61a3a01e3fd81c5a
```

Форматированный micro fixture gate revision `e0961a73` побайтово совпадает на обеих сторонах:
SHA-256 `3FFEE32A8EB756E372C6349D6E1BD97413AA433929AFE55598E1A90EDA6311B9`.
От timed fixture `771980A9…` отличается только переносом одного assertion;
timed loops и controls не менялись, timings повторно не объявляются новым прогоном.

Finite-oracle follow-up code revision `b1c8c2fea59814fc70295403d80e4e4fce29df16`
прошла `check-changed --base e7dadda5c40283e40450e7d576730c670e58f8f2`
тем же copied runner с обычным host lock, exit 0. Изменён flui-animation и
23 dependents: fmt, strict all-target clippy, engine/testing, 687/687 nextest
за 169.046 s (19 configured skips, один slow завершён), strict private rustdoc,
457 doctests passed / 159 ignored, Windows optional source checks, wasm и
34 library + 34 tests/benches/examples feature cases. Apple/Android checks
локально пропущены с теми же SDK/tool limits. Финальная receipt-документация
не меняет этот code revision; её static checks и PR CI — отдельные проверки.

### CI

[Main CI](https://github.com/vanyastaff/flui/actions/runs/38255767900) на production
revision завершён успешно. Это receipt исходной реализации, а не будущего SHA
изменённых тестов и документации.
[Validation CI](https://github.com/vanyastaff/flui/actions/runs/38282787508) на
`e7dadda5c40283e40450e7d576730c670e58f8f2` завершён успешно: checks, clippy,
test, test-nested, test-trybuild, test-features, live-smoke, doc, deps, wasm-check,
Windows/macOS/Android/iOS cross-typecheck и агрегатор CI. Configured skipped jobs
не объявляются выполненными. Этот receipt не покрывает последующие изменения
benchmark oracle; A9 ожидает CI их финальной revision.
[Weekly](https://github.com/vanyastaff/flui/actions/runs/38253895275) на
`f8759e33d6729434d0b2b8ffac21411c0679cc59` имеет failures в latest-deps,
release-clippy и CLI live-web; он не объявляется зелёным по результату main CI.

Все 110 market-ID сверяются с [актуальной матрицей](readiness-status.md).
Scope исключений сохраняется; authoring API не требует вымышленного штатного caller.
Source query `rg -n 'AnimatableExt|FloatTween|ColorTween' src crates packages examples`
не находит старых имён; canonical root/prelude и SDK imports сохранены.
Выпускной checklist находится в [плане](readiness-plan.md).
