# flui-animation — статус, контракты и свидетельства

- **Статус:** A0–A9 завершены в принятом scope; code revision `71f13790b` прошла validation CI.
  Каждой из 110 строк [требований](requirements.md) назначен исполняемый контракт, сохранённый
  авторский API либо согласованное исключение («Контракты требований» ниже); границы других
  native-платформ указаны в «Выпускных свидетельствах».
- **Требования:** [requirements.md](requirements.md); подход и решения — [design.md](design.md).

## Контракты требований

Источник — 110 строк [requirements.md](requirements.md); исторический маршрут сохранён в
[readiness-matrix.md](https://github.com/vanyastaff/flui/blob/deb9faea20fe6beb23c9ea0724f5a75e0f42595f/docs/plans/specs/flui-animation/readiness-matrix.md). Здесь каждой строке назначен текущий
контракт, наблюдаемое доказательство либо согласованное исключение. Это не
обещание выполнения каждой платформы и каждого workload на текущем компьютере.

«Сценарий» означает исполняемый контракт текущего production-пути. «Авторский»
означает явно сохранённое владельцем расширение с публичной приёмкой.
«Вынос» ссылается на scope [design.md](design.md), уточнённый
тематическими requirements. «Внешняя проверка» сохраняет указанную границу
native/GPU/измерений и не превращает старый результат в current-head receipt.

Имена тестов разрешаются командой `rg -n '<имя>' crates packages`.
Численные проверки находятся в `crates/flui-animation/tests/contracts`, mounted
сценарии — в `flui-widgets/tests`, пакетах и runtime. Терминальные счётчики и
внутренний presentation gate требуют документированного private seam; это
дополнение к публичным owner/frame сценариям, а не их замена.

### Время

| ID | Disposition | Контракт и доказательство |
|---|---|---|
| M-TIME-1 | Сценарий | Чистое время запуска; `frame_partitions` сравнивает painted opacity и layout spring/curve при разных разбиениях; repeat и spring имеют отдельные partition properties |
| M-TIME-2 | Сценарий | `one_pump_shares_its_selected_time_across_parent_and_child`: host clock меняется при выборе и внутри callback, оба registry сохраняют выбранный stamp и original futures |
| M-TIME-3 | Сценарий | `backwards_raw_time_holds_the_timeline`, stale manual samples и robustness; `Duration` исключает NaN/∞ до frame admission, повторный stamp не повторяет outcome |
| M-TIME-4 | Сценарий | `has_running` управляет frame demand; mounted settlement прекращает paint, owner teardown снимает seat |
| M-TIME-5 | Сценарий | `gated_presentations_hold_samples_then_catch_up_when_visible`: hidden и scheduler-disabled удерживают sample/status/outcome/frame count для tween, spring и двух repeat; resume догоняет wall time |
| M-TIME-6 | Сценарий | Public Vsync/TickerMode семьи различают mute и pause: mute не сэмплирует, уже якорённое время догоняется; новый запуск якорится в первый разрешённый кадр |
| M-TIME-7 | Сценарий | `rate_change_rebases_without_a_jump` и независимые window clocks; DevTools меняет clock выбранной презентации, процесс-глобального dilation нет |
| M-TIME-8 | Сценарий | `HeadlessBinding`/`pump_for`, MotionClock и public frame traces используют виртуальное typed time; mounted partition сценарии исполняют всю цепочку |
| M-TIME-9 | Сценарий | `Duration` → `AnimationTime` → `FrameTick`; nanosecond reversal и exact stepping проверяются без raw-f64 frame admission |
| M-TIME-10 | Сценарий | `curved_run_velocity_is_the_curve_slope`, `controller_velocity_boundaries`, независимые spring x/dx и реальные retarget consumers |
| M-TIME-11 | Вынос | Preferred frame rate/throttling исключены владельцем |
| M-TIME-12 | Сценарий | Motion-clock R4 уточняет рынок: конечный rate ≥ 0, reverse отдельной командой; tween/curve/repeat/spring проверяют paused gap, seam, velocity и subsequent delivery; Snackbar — реальный caller |

### Прерывания

| ID | Disposition | Контракт и доказательство |
|---|---|---|
| M-INT-1 | Сценарий | `retarget_is_c0_and_c1_at_the_seam` и mounted opacity/padding/container сравнивают текущие painted/layout значения, не configuration endpoints |
| M-INT-2 | Сценарий | Scalar/vector physical spring переносит seam velocity; owning motion и mounted geometry проверяют C1 |
| M-INT-3 | Сценарий | `a_time_defined_spring_inherits_velocity`: duration/bounce — физическая параметризация; narrow target и входящая скорость сравниваются с независимым ОДУ. Нулевая solver velocity нарушает position oracle |
| M-INT-4 | Сценарий | `forward_from_mid_scales_run_duration`, sub-EPSILON distance и MAX-duration refusal/continuation; арифметика не оставляет частично принятый run |
| M-INT-5 | Сценарий | `reversal_shortens_by_the_eased_fraction`, ранний разворот и mounted implicit/Hero retrace сохраняют seam и сокращённый срок |
| M-INT-6 | Сценарий | `fling_across_hands_the_gesture_velocity_to_the_spring` и Drawer/Dismissible/back gesture используют фактическую протяжённость; mounted assertions читают реальные координаты |
| M-INT-7 | Сценарий | Controller reverse, direction-locked CurvedAnimation и Cupertino route доставляют активность/phase/outcomes при развороте |
| M-INT-8 | Сценарий | `set_value` — seek без нового run; старый future получает cancel, NaN/bounds/disposal проверяются отдельно |
| M-INT-9 | Сценарий | Новый run отменяет прежний; command/source/delivery таблицы проверяют observable status до cancel, замену из listener и следующий здоровый run |
| M-INT-10 | Сценарий | Zero-duration, native/custom simulation, finite repeat и owner retirement различают success/cancel; status/waker/reentry семьи сохраняют once-only outcome |
| M-INT-11 | Авторский | `finish` сохраняет original future; robustness покрывает finite endpoints, parked work, повторный вызов и failure custody; authored page проверяет реальный paint/disposal (ADR-0197) |
| M-INT-12 | Сценарий | Hostile curve/simulation/source reentry не публикует stale, nonfinite или out-of-bounds sample; независимые original/incoming futures и subsequent frame проверены |
| M-INT-13 | Вынос | Условие выполнено для repeated retarget: 120 Curve/Spring прерываний scalar/Offset плюс painted opacity и layout padding C0/C1. Независимые additive layers не заявлены |

### Физика

| ID | Disposition | Контракт и доказательство |
|---|---|---|
| M-PHY-1 | Сценарий | `spring_matches_analytic_reference` сверяет три режима с независимыми фиксированными x/dx и textbook formulas |
| M-PHY-2 | Сценарий | Critical-neighbour continuity, extreme finite domain и heavy-damping slow root; промежуточный overflow отвергается при admission |
| M-PHY-3 | Сценарий | Checked stiffness/damping-ratio constructor; Scroll/Page physics — callers, constructor table проверяет refusal |
| M-PHY-4 | Сценарий | `perceptual_springs_match_published_conversions` и `perceptual_branches_are_consistent`; Duration/bounce/response constructors валидируют область. Эквивалентность частной реализации Apple не заявлена |
| M-PHY-5 | Вынос | Presets и обратные getters удалены по решению владельца; unused API не восстанавливается |
| M-PHY-6 | Сценарий | Private validated fields, fallible simulation admission; `animated_value_never_latches_non_finite` и atomic retarget refusal |
| M-PHY-7 | Сценарий | Conservative rest используется `is_done`, fling/settlement и SpringCurve; `rest_time_is_conservative_and_tight` проверяет границу. Сам rest_time остаётся private по physics R17 |
| M-PHY-8 | Сценарий | Scroll DPR tolerance проверен реальным scroll producer; typed component thresholds — отдельный контракт ADR-0186, не вывод из общего epsilon |
| M-PHY-9 | Сценарий | `fling_completes_on_the_frame_that_reaches_the_bound` проверяет endpoint, settled phase и original future без субпиксельного хвоста |
| M-PHY-10 | Сценарий | Friction x/dx и конечный предел; near-unit drag имеет независимый constant-velocity oracle и remaining-glide rest boundary |
| M-PHY-11 | Сценарий | `friction_preserves_small_decay_and_position_time_roundtrips` различает stationary/unreachable/past-limit queries; unused `through` удалён по physics R14 |
| M-PHY-12 | Сценарий | Friction предел используется BouncingScrollSimulation; публичная handoff таблица проверяет сохранённую скорость, mounted scroll достигает rest |
| M-PHY-13 | Сценарий | `bouncing_simulation_hands_friction_to_spring_at_the_edge` сравнивает x/dx по обе стороны аналитического crossing, overshoot и exact rest; mounted bouncing fling проверяет consumer |
| M-PHY-14 | Вынос | Smoothing удалён по physics R15 |
| M-PHY-15 | Вынос | Тот же принятый отказ от smoothing; нет оставленного полиномиального follow helper |
| M-PHY-16 | Сценарий | Nonsettling built-in spring и overflow coefficients отвергаются; Gravity удалена. Custom endless Simulation остаётся явным extension contract с bounded settlement fallback |

### Кривые

| ID | Disposition | Контракт и доказательство |
|---|---|---|
| M-CRV-1 | Сценарий | Checked Cubic new/try_new/serde имеют одну admission policy; constructor controls private, Cupertino использует catalog |
| M-CRV-2 | Сценарий | Независимая Bezier bisection и vertical-tangent analytic inverse проверяют output error, не только обратный x |
| M-CRV-3 | Сценарий | Catalog фиксирует контрольные точки и ссылки; реальные route/button callers, README/GUIDE отражают easing форму |
| M-CRV-4 | Сценарий | Public catalog family проверяет точные 0/1 всех сохранённых curves; SawTooth/ReverseCurve/Threshold удалены |
| M-CRV-5 | Сценарий | Interval public table и production Hero subrange; finite ordered interval admission |
| M-CRV-6 | Сценарий | Steps четыре jump modes и checked count/serde; production indicator. Before flag исключён принятым отказом от полной WAAPI timing model |
| M-CRV-7 | Авторский | PiecewiseLinear canonical stops, missing/decreasing inputs и right-continuous interior jump; numeric/serde table и painted Hero retrace. CSS parsing/extrapolation исключены FLUI Curve-контрактом |
| M-CRV-8 | Вынос | Cubic exterior extrapolation исключена; экстраполяция `Lerp` значения — другой контракт |
| M-CRV-9 | Авторский | Timed cubic Keyframes заменяет старые Catmull APIs; `authored_cubic_hero_geometry_is_painted_and_retraced` проверяет factory, independent painted geometry и reverse |
| M-CRV-10 | Авторский | SpringCurve — immutable analytic sampling по conservative duration, не frame table; три ODE режима, typed refusal, equality/original future и mounted Hero overshoot/retrace |
| M-CRV-11 | Сценарий | FLUI Curve clamp/NaN/exact endpoint policy; Tween extrapolation и integer clamping deliberate. Hostile-domain family проверяет различие этих контрактов |
| M-CRV-12 | Сценарий | `curve_parameters_reject_invalid_input` сравнивает enum causes у new/try_new/numeric serde; subnormal midpoint normalization имеет отдельный RescalingOutOfRange |

### Интерполяция

| ID | Disposition | Контракт и доказательство |
|---|---|---|
| M-INTP-1 | Сценарий | Generic `Tween<T: Lerp>`; `integer_tweens_interpolate_across_the_full_range` задаёт independent round/floor extrema, NaN и clamps |
| M-INTP-2 | Сценарий | Premultiplied Oklab numeric references и typed color motion сохраняют hue при fade, geometry/color идут через реальные implicit callers |
| M-INTP-3 | Внешняя проверка | Continuous Color и engine paint/filter маршруты (ADR-0194); GPU readback различает fractional alpha в Tween/group/DecorationImage. Windows required-GPU run 2026-10-12 на production 61ea71338: engine 68/68 и facade table 1/1, без skips; [release receipt](#выпускные-свидетельства). Другие adapters/platforms и GPU latency budget этим не доказаны |
| M-INTP-4 | Вынос | OkLCh/hue policies исключены владельцем |
| M-INTP-5 | Сценарий | Interpolation I1–I6/I14 уточняют collapsed scale, borrowed rotation, skew/perspective и nondecomposable discrete fallback; foundation oracle и mounted AnimatedContainer |
| M-INTP-6 | Сценарий | `animated_rotation_takes_the_requested_arc` различает Numeric/Shorter и многооборотную траекторию по реальному transform |
| M-INTP-7 | Сценарий | Fixed AnimationVector + owning AnimatedValue, vector sample commit; PropertyMotion и coordinated updates не требуют per-component owners |
| M-INTP-8 | Сценарий | `two_way_converter_derive_contract` использует nested external DTO и настоящий retarget; SDK nested_motion fixture импортирует trait+derive через prelude |
| M-INTP-9 | Сценарий | Component thresholds сохраняют единицы; geometry/scalar заканчивают в разные сроки, invalid thresholds не меняют оба owners и их futures (ADR-0186) |
| M-INTP-10 | Вынос | Discrete opt-in surface исключён владельцем |

### Композиция

| ID | Disposition | Контракт и доказательство |
|---|---|---|
| M-CMP-1 | Сценарий | Keyframes timed segments, pure elapsed/progress sample; mixed hold/jump/cubic/eased joins имеют independent values/derivatives, indicators реально рисуют tracks |
| M-CMP-2 | Сценарий | Leading/trailing hold задают delay/end hold; indicator timing и keyframes table |
| M-CMP-3 | Сценарий | `repeat_contract`, policy и paused repeat семьи различают finite/infinite/autoreverse, skipped cycles, zero period, final leg и owner cancellation |
| M-CMP-4 | Сценарий | Indicator head/tail/turn/sweep читают один parent progress; grouped track/paint assertions |
| M-CMP-5 | Сценарий | Stagger origin/offset table и Cupertino cyclic indicator; list-window stagger отдельно исключён composition scope |
| M-CMP-6 | Вынос | Полная WAAPI timing model исключена владельцем |
| M-CMP-7 | Вынос | CompoundAnimation и его exports удалены; арифметический комбинатор не оставлен невключённым |
| M-CMP-8 | Сценарий | Exact crossing вместо EPSILON hop; public value/status/reentry плюс mounted Navigator near-zero progress и subsequent live-source activity |
| M-CMP-9 | Сценарий | Directional curve latch и разные entrance/exit duration; mounted PageRoute проверяет painted exit, registry retirement и позднюю navigation |
| M-CMP-10 | Сценарий | Proxy parent commit предшествует callouts; child-process public rows заменяют из read/status/removal, проверяют порядок и следующий успешный swap |
| M-CMP-11 | Вынос | Условие выполнено для двух согласованных случаев: циклические hold+repeat Material bars и implicit multi-property state retarget; произвольный PhaseAnimator не заявлен |

### Доступность

| ID | Disposition | Контракт и доказательство |
|---|---|---|
| M-A11Y-1 | Внешняя проверка | Public host→runtime→MediaQuery/FrameTick проверяет first/live preference и wake-before-settle; Windows physical preference producer исполнен 2026-10-12, включая независимое сравнение AnimationsEnabled. Реальное переключение OS setting и другие native platforms этим не доказаны |
| M-A11Y-2 | Сценарий | FollowSystem/Reduce/Full override имеет девять комбинаций и first-root/public runtime tests, preference принадлежит презентации |
| M-A11Y-3 | Сценарий | Essential scroll/refresh Preserve, indicators Normal; reduced mounted fixtures различают motion и static feedback |
| M-A11Y-4 | Сценарий | Finite oneshot/repeat settle в declared endpoint, infinite repeat parks pending; Full/rebind возобновляет, completion/status не пропадают |
| M-A11Y-5 | Внешняя проверка | Validated host duration scale и public runtime updates; zero scale settles next frame, invalid input refuses, tiny scale saturates. Native backend execution имеет отдельную границу |
| M-A11Y-6 | Вынос | Отдельный cross-fade preference исключён владельцем |

### Владение

| ID | Disposition | Контракт и доказательство |
|---|---|---|
| M-OWN-1 | Сценарий | Run/sample generation fences; source family replace/stop/nested tick сохраняет latest owner, отменяет displaced future и отвергает stale nonfinite output |
| M-OWN-2 | Сценарий | DrivenController и AnimatedValue единолично владеют seat/run; public owner lifetime и mounted unmount оставляют surviving observers без движения |
| M-OWN-3 | Сценарий | Disposed command/callback admission закрыт, поздние captures отпускаются; next healthy peer/source продолжает работу |
| M-OWN-4 | Сценарий | VsyncUpdate/rebind и did_change_dependencies; GlobalKey сохраняет RenderId/sample, снимает старый seat, держит seam под mute и освобождает destination при unmount |
| M-OWN-5 | Сценарий | Activity независима от directional phase; wrappers отражают installed run, switch inactive→live→complete→reverse→dispose, fixed values inactive при каждой phase |
| M-OWN-6 | Сценарий | Weak source-bound status guards и shared ParentLinks; last-wrapper retirement убирает relays, surviving source молчит; failure custody сохраняет первый отказ |
| M-OWN-7 | Сценарий | Source/registration identity и terminal-counter refusal; public guards не снимают чужую регистрацию. Constant inert token не конфликтует с core notifier и не имеет global counter |
| M-OWN-8 | Сценарий | Status/Vsync first-failure custody доставляет healthy controller/child peers и recoverable tail; single/competing/next-operation cases |
| M-OWN-9 | Сценарий | Removed listeners пропускаются, reentrant commits идут FIFO, teardown commits перед capture Drop; public status/proxy/subscription matrices |

### Интеграция

| ID | Disposition | Контракт и доказательство |
|---|---|---|
| M-INTG-1 | Сценарий | Transition matrix/paint/hit/semantics fixtures и actual Drawer/Dismissible geometry читают output без per-tick rebuild; sample points различают transform |
| M-INTG-2 | Сценарий | AnimatedSize intermediate layout widths, target, migration/unmount и render-object harness; completion signal изменяется вне build и не переигрывается |
| M-INTG-3 | Сценарий | Hero flight overlay/current-rect factory, airborne interruption и authored tracks; painted bounds, reverse, landed source и fresh later pop |
| M-INTG-4 | Сценарий | AnimatedSwitcher удерживает outgoing subtree до completion, мигрирует registrations и отпускает их при teardown |
| M-INTG-5 | Вынос | FLIP сверх существующего Hero исключён владельцем |
| M-INTG-6 | Сценарий | Scrollable/Refresh release используют captured gesture profile; изменения между Down/Up не меняют текущую admission, next contact использует новые thresholds |
| M-INTG-7 | Вынос | Platform spline/deceleration models исключены владельцем |
| M-INTG-8 | Сценарий | Unbound owner settles explicitly; detached TickerMode, implicit и native fling fixtures не оставляют pending невидимое движение |
| M-INTG-9 | Сценарий | DevTools agent endpoint принимает actual protocol request и меняет clock выбранного window; runtime playback/paused-step family |
| M-INTG-10 | Вынос | Новый configurable hit surface исключён; существующие transform/hit hooks остаются обязательными и проверяются transitions |

### Качество

| ID | Disposition | Контракт и доказательство |
|---|---|---|
| M-QLT-1 | Сценарий | Public curve catalog/properties + independent references: endpoint, admitted output/overshoot и monotonic families; constructor admission отдельно от input progress |
| M-QLT-2 | Сценарий | Independent spring regime and friction/crossing oracles; smoothing удалён, его старые properties не считаются доказательством нового поведения |
| M-QLT-3 | Сценарий | Mounted partition + pure curve/spring/repeat properties; frame count не подменяет equal output/outcome assertions |
| M-QLT-4 | Внешняя проверка | Paired CPU runs 2026-10-12 измерили 17 registry, 21 primitive и 32 mounted workloads, с 96 moving scenes и отдельным idle; allocation requests/bytes измерены отдельно. Параметры и revisions в разделе «Выпускные свидетельства»; speedup/GPU budget не заявлен |
| M-QLT-5 | Сценарий | `a_steady_state_frame_allocates_nothing`: 10 000 advancing warmed scalar/vector/keyframes/relay ticks, calls+bytes и positive control; idle retirement отдельно. Whole mounted frame не объявлен безаллокирующим |
| M-QLT-6 | Сценарий | README/GUIDE/PERFORMANCE и crate architecture обновлены: canonical imports, ownership, curve/physical spring distinction, retained authoring и точные границы старых измерений |
| M-QLT-7 | Сценарий | Crate prose включено через include_str doctests; scope gate выполняет private rustdoc/doctests и compile examples. Ignored blocks/platform docs не выдаются за исполненные |
| M-QLT-8 | Сценарий | Один animation_it binary + общий table runner; allocation/child-process/terminal private seams имеют отдельную обоснованную topology |
| M-QLT-9 | Сценарий | Canonical root/prelude и consumer disposition в crate architecture; aliases/unwired pure adapters удалены, actual callers мигрированы. Builder/finish/chosen-phase/cubic/authored easing сохраняют explicit owner-approved authoring scope |
| M-QLT-10 | Сценарий | Repository markers/checks gate выполняется; requirement/process IDs остаются только в approved specs/history |
| M-QLT-11 | Сценарий | Numeric construction/serde table и authored easing исполняются с all-features; compilation feature alone не заменяет decode assertions |

### Что доказательство не расширяет

Local checks, исполненный required-GPU gate, Windows preferences и paired CPU
measurements имеют отдельные датированные [receipts](#выпускные-свидетельства).
Windows input/UIA также исполнены с PASS; один adapter/host не
расширяет доказательство на другие платформы. Public API cleanup намеренно несовместим с прежними module/alias paths;
SemVer assessment и инструкции migration относятся к этому изменению, а не к
восстановлению параллельного legacy API.

Смена только duration у implicit curve дополнительно проверяется
`changing_only_the_duration_preserves_motion_and_sets_a_new_deadline`, а смена
только curve — `changing_only_the_curve_keeps_the_existing_deadline`. Эти случаи
не подменяют физический velocity-inheritance контракт M-INT-3.

## Выпускные свидетельства

Дата проверки: 2026-10-12. Production revision:
`61ea713380a449777ffa791b61a3a01e3fd81c5a`.
Native assertion revision: `a73d351ce3ae9a6a0df3119d95304b0b80f09426`.
Validation code revision: `71f13790b43df65be2ef441aaca904aac6c353f8`.
Framework production-код относительно указанной базы не менялся; изменены
assertions, benchmark validation и foreground helper инструмента проверки.
A8 завершён в указанных границах Windows/native/GPU и измеренных workloads.
A9 завершён в принятом scope: CI указанной code revision прошёл успешно.
CI production-базы, локальный gate и CI каждой validation revision — отдельные receipts.

### Среда и метод

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

### Измерения

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

#### Mounted frames

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

#### Animation primitives

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

#### Allocation boundary

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

### Native и GPU

`cargo xtask gpu-test` завершился с exit 0: engine 68/68 (260.222 s, один slow),
facade readback table 1/1 (15.127 s), без skips. Это реальное исполнение shader,
layer/filter/damage и fractional-alpha readbacks; type-check не заменял запуск.
`cargo xtask device windows-input` и `cargo xtask device windows-a11y` завершились
с exit 0: `INPUT=PASS`, `A11Y=PASS`. Первый input attempt безопасно завершился
`CANNOT_VERIFY` при отказе foreground. Tooling теперь допускает bounded 30 s
для ручного фокуса; повторный запуск действительно исполнил input, без обхода
foreground/hit checks. В успешном запуске дополнительный wait не понадобился.
Driver запускает counter через `Session::start_for`: полный nominal budget
discovery, 30 s foreground grace, control, focus polling и result waits составляет
58 s; self-termination guard установлен на 116 s с запасом для scheduling/UIA.
Перед принятием foreground и перед press проверяются PID текущего HWND и
несигнальное состояние process handle запущенного child. Общие input helpers
Notes используют ту же проверку; release уже отправленного press сохраняется.
`foreground_recovery_requires_the_live_probe_owner` исполнил late focus,
owner loss during wait, foreign foreground handle и timeout. На прежнем polling
поведении case упал именно на owner loss и foreign handle; после исправления
43/43 xtask unit tests прошли. Повторный native input run дал `INPUT=PASS`
(miss 0, hit 1, Tab/Enter 2), снова без дополнительного wait. Предыдущий attempt
увидел count 2 после control и завершился `INPUT=FAIL`; его результат не считается
acceptance evidence. Live recovery после позднего ручного фокуса не подтверждён.
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

### CI и scope

#### Исторические 32 red-сценария

29 имён исходных случаев сохранены в текущих animation tests. Их старые red
receipts не заменяются новым чтением исходников; текущий all-feature запуск
проверяет зарегистрированные семейства. Три изменённых шва:

| Исторический случай | Текущий контракт |
|---|---|
| `nan_frame_time_is_skipped` | Frame admission принимает `Duration`, где NaN не представим; `finite_clock_time_is_anchored` проверяет правильную finite-time траекторию. Raw-f64 API не восстанавливается |
| `frame_scheduled_hook_may_read_the_controller` | `wake_failure_preserves_delivery_and_recovery`: requester читает committed `is_animating`/status, после wake failure healthy tail доставляется и следующий run завершается |
| `frame_scheduled_hook_may_query_vsync` | `wake_replacement_retires_captures_outside_borrows`: requester заменяет hook того же registry, а outgoing capture проверяет `has_running` вне borrow; recovery и следующая доставка сохранены |

`rg -n '#\[ignore' crates/flui-animation/tests` не находит отключённых контрактов.

#### Локальный gate

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

#### CI

[Main CI](https://github.com/vanyastaff/flui/actions/runs/38255767900) на production
revision завершён успешно. Это receipt исходной реализации, а не будущего SHA
изменённых тестов и документации.
[Validation CI](https://github.com/vanyastaff/flui/actions/runs/38282787508) на
`e7dadda5c40283e40450e7d576730c670e58f8f2` завершён успешно: checks, clippy,
test, test-nested, test-trybuild, test-features, live-smoke, doc, deps, wasm-check,
Windows/macOS/Android/iOS cross-typecheck и агрегатор CI. Configured skipped jobs
не объявляются выполненными. Этот receipt не покрывает последующие изменения
benchmark oracle и Windows input driver.
[Final code CI](https://github.com/vanyastaff/flui/actions/runs/38306186115) на
`71f13790b43df65be2ef441aaca904aac6c353f8` завершён успешно: checks, clippy,
test, test-nested, test-trybuild, test-features, live-smoke, doc, deps, wasm-check,
Windows/macOS/Android/iOS cross-typecheck и агрегатор CI. Это receipt конечных
assertions, finite benchmark oracle и исправленного probe lifetime/ownership.
Локальный `check-changed --base 8e1f7b36deb5f57f3c06081144ebc3e8b08ea6e8`
прошёл на этих исходниках: clippy, 47/47 xtask tests, strict rustdoc и Windows
type-check. Configured skips не считаются выполненными, а native recovery после
позднего ручного фокуса по-прежнему не объявляется проверенным live-путём.
Последующее закрытие checklist меняет только эти выпускные документы;
его static checks и docs-only CI являются отдельной проверкой.
[Weekly](https://github.com/vanyastaff/flui/actions/runs/38253895275) на
`f8759e33d6729434d0b2b8ffac21411c0679cc59` имеет failures в latest-deps,
release-clippy и CLI live-web; он не объявляется зелёным по результату main CI.

Все 110 market-ID сверяются с [актуальной матрицей](#контракты-требований).
Scope исключений сохраняется; authoring API не требует вымышленного штатного caller.
Source query `rg -n 'AnimatableExt|FloatTween|ColorTween' src crates packages examples`
не находит старых имён; canonical root/prelude и SDK imports сохранены.
Выпускной checklist находится в [плане](design.md).

## Исторический реестр дефектов

Реестр на базе `main` 9a4daa3ed от 2026-10-06; `file:line` ниже — на этот коммит. Старые D-ID
сохранены для трассировки; их наличие в таблице не означает текущий дефект.

### Дефекты (без дублей, по убыванию серьёзности)

H — высокая, M — средняя, L — низкая. «Не подключено» — production-вызовов нет.

| ID | Дефект | Сер. | Где | Тема |
|----|--------|------|-----|------|
| D-01 | Panic слушателя статуса обрывает раздачу; переход потерян навсегда (маркер уже сдвинут); в `Vsync::tick_all` пропускает остальные контроллеры кадра, включая родительские после дочернего реестра | H | controller.rs:2371-2376, 2473; proxy.rs:43-45; switch.rs:260, 270-272; vsync.rs:462-466 | listener-delivery |
| D-02 | `AnimationSwitch` читает `value()`/`status()` родителей под своим нереентерабельным mutex | H | switch.rs:294-295, 445, 450, 492-499 | listener-delivery |
| D-03 | Lerp `Matrix4` даёт NaN при нулевом масштабе на конце; skew/perspective теряются | H | flui-foundation matrix4.rs:173-181 | interpolation |
| D-04 | Curved-прогон публикует выход кривой без проверки конечности и clamp | M-H | controller.rs:1921-1940, 2012 | controller-robustness |
| D-05 | Reduce motion нигде не проведён | M | status.rs:108-138; flui-app runtime.rs:97; media_query.rs:47-87 | motion-clock |
| D-06 | Реентерабельные смены статуса приходят не по порядку | M | controller.rs:2411-2418 | listener-delivery |
| D-07 | Снятый во время раздачи (или `dispose()`) слушатель статуса всё равно вызывается | M | controller.rs:2372; proxy.rs:36-46; switch.rs:270 | listener-delivery |
| D-08 | Implicit-анимации при retarget стартуют с 0: теряется скорость (C¹) | M | flui-widgets implicitly_animated.rs:147-152 | retarget |
| D-09 | Hop в switch не сообщает слушателям статус нового поезда | M | switch.rs:313-345 | listener-delivery |
| D-10 | `ProxyAnimation::set_parent` меняет три `RwLock` неатомарно | M | proxy.rs:193-226 | listener-delivery |
| D-11 | `is_animating` различается через обёртки | M | animation.rs:88; controller.rs:2715-2728 | controller-robustness |
| D-12 | Перцептивные конструкторы пружины принимают невалидный вход в release; pub-поля обходят проверку | M | simulation.rs:121-128, 183-225 | physics |
| D-13 | Scroll-симуляции используют допуск в долях единицы на пикселях | M | simulation.rs:42-46, 821-827, 922-929 | physics |
| D-14 | `FLING_TOLERANCE` мёртв: fling заканчивается ~0.26 с после границы | M | controller.rs:27-31, 1613-1628 | physics |
| D-15 | `BouncingScrollPhysics` без перехода friction→spring: fling в край останавливается намертво | M | flui-widgets scroll_physics.rs:349-355 | physics |
| D-16 | `RefreshIndicator` пересобирает поддерево на каждый пиксель прокрутки | M | refresh_indicator.rs:449 | integration |
| D-17 | Валидация кривых обходится через pub-поля и serde | M | curve.rs:140-148, 196-200, 229-239, 335-350, 586-590 | curves |
| D-18 | `ReverseCurve`/`Curve::reversed` нарушает контракт 0→0, 1→1 | M | curve.rs:51-60, 977-996 | curves |
| D-19 | `CatmullRomCurve` игнорирует x, концы не 0/1, tension задокументирован наоборот, panic на пустых точках; `SawTooth(1)=0` | M | curve.rs:791-861, 131-134 | curves |
| D-20 | `lerp_oklab` без premultiplied alpha | M | flui-painting color.rs:610-623 | interpolation |
| D-21 | Implicit-виджеты без `VsyncScope` не анимируются, docs обещают fallback | M | implicitly_animated.rs:72-74; vsync_scope.rs:21-23; ticker_mode.rs:180-196 | ownership |
| D-22 | `VsyncScope` никогда не уведомляет: замена/перенос реестра замораживает контроллеры | M | vsync_scope.rs:76-80; ticker_mode.rs:160-166 | ownership |
| D-23 | `friction.through` паникует на underflow drag, с `ve=0` не завершается | M | simulation.rs:699-708, 730 | physics |
| D-24 | `SmoothDamp`: panic при отрицательном `max_speed`, зависимость от частоты кадров, вечный NaN | M | smoothing.rs:144, 160-163, 187, 193-196 | physics |
| D-25 | Time dilation масштабирует весь прогон и глобальна на процесс | L-M | controller.rs:1897, 2521; scheduler config.rs:9 | motion-clock |
| D-26 | `Duration::MAX` паникует в `mul_f64` под lock после частичной мутации | L-M | controller.rs:2505-2515 | controller-robustness |
| D-27 | Нет типизированной идентичности слушателей: id с 1 в каждом реестре, общий тип value/status, счётчик может переполниться | L-M | controller.rs:658, 2696-2697; proxy.rs:30; constant.rs:15 | listener-delivery |
| D-28 | Слушатели статуса через `Compound`/`Reverse` живут на родителе: цикл ссылок | L-M | compound.rs:216-223; reverse.rs:91-108 | listener-delivery |
| D-29 | `Switch::dispose` держит `on_switched` и слушателей | L-M | switch.rs:402-430; transition_route.rs:443-450 | listener-delivery |
| D-30 | Скорость fling в Dismissible — фиксированный масштаб 1/300 | L-M | dismissible.rs:107, 1175, 1180 | retarget |
| D-31 | После `dispose` работают `set_value` и `add_*listener`; value-слушатели не очищаются | L | controller.rs:2151-2214, 2693-2700 | controller-robustness |
| D-32 | `on_frame_scheduled` вызывается под guard контроллера: инверсия lock и полузапущенный прогон | L | controller.rs:2319-2339; vsync.rs:340-351 | controller-robustness |
| D-33 | Неконечный `now_secs` навсегда закрепляет прогон на NaN | L | vsync.rs:484-489 | motion-clock |
| D-34 | `tick()` отматывает Vsync-прогон к началу | L | controller.rs:1854-1860 | controller-robustness |
| D-35 | Проверка цикла в `attach_child` — TOCTOU; дубли регистрации тикаются дважды | L | vsync.rs:225, 232 | controller-robustness |
| D-36 | `controller.velocity()` игнорирует кривую | L | controller.rs:1765-1787 | retarget |
| D-37 | Солвер cubic проверяет допуск только по x (ошибка y до 9e-3) | L | curve.rs:271-312 | curves |
| D-38 | Elastic-кривые прыгают на концах; политика NaN разная у кривых и tween'ов | L | curve.rs:606-704; tween_types.rs:117, 144 | curves |
| D-39 | Переполнение в пружине (`4mk`, `x(+inf)`=NaN); `AnimatedValue` держит NaN навсегда | L | simulation.rs:497, 572; spring.rs:113-181 | physics |
| D-40 | `Bounded`/`Clamped` паникуют на неупорядоченных или NaN-границах | L | simulation.rs:859-873, 889-892, 934 | physics |
| D-41 | Slide/Scale/Rotation-переходы, drawer, dismissible анимируют transform пересборкой | L | slide_transition.rs:111-117; drawer.rs:735-738; dismissible.rs:669-671 | integration |
| D-42 | Владение у потребителей: sliver header не освобождает контроллер, таймер snackbar перезаписывает регистрацию, `set_snap_controller` не переподписывается | L | sliver_persistent_header.rs:292-301; scaffold_messenger.rs:549-569; objects sliver_persistent_header.rs:1067-1069 | ownership |
| D-43 | `#[derive(Animatable)]` реализует `TwoWayConverter`; гигиена; принимает unit-структуры | L | flui-macros derive_animatable.rs:75-120 | derive |
| D-44 | Switch шлёт лишние уведомления значения; `Compound::divide` публикует NaN | L | switch.rs:78-79, 269; compound.rs:85-95 | listener-delivery |
| D-45 | Бенч `tick_at` меряет остановленный контроллер; таблицы PERFORMANCE.md не измерены или неверны | L | animation_bench.rs:133-145; PERFORMANCE.md:27, 160-194, 477-490 | quality |
| D-46 | Раскладка тестов: единственный бинарь `tests/derive_animatable.rs`, тесты публичного API в `src`, мёртвый `SERIAL`, чтение приватного поля, process markers | L | controller_tests.rs:12-20, 230, 571 | quality |
| D-47 | Ошибки несут `String`; `TickerNotAvailable` не создаётся | L | error.rs:28-95 | controller-robustness |

### Темы

| Тема | Форма | ADR | Закрывает |
|------|-------|-----|-----------|
| listener-delivery | спека | да (контракт наблюдателя, уточняет ADR-0064 §4) | D-01, 02, 06, 07, 09, 10, 27, 28, 29, 44 |
| controller-robustness | спека | да (Vsync — единственные часы контроллера; заменяет ADR-0064 §6) | D-04, 11, 26, 31, 32, 34, 35, 47 |
| motion-clock | спека | да (time dilation и motion policy у часов презентации; выход `TIME_DILATION` из ADR-0097) | D-05, 25, 33 |
| physics | спека | нет | D-12, 13, 14, 15, 23, 24, 39, 40 |
| retarget | спека | да (retarget сохраняет значение и скорость) | D-08, 30, 36 |
| ownership | спека | да (владеющий handle контроллера) | D-21, 22, 42; ручные register/unregister/dispose в ~14 файлах |
| integration | спека | нет | D-16, 41 |
| curves | design + tasks | нет | D-17, 18, 19, 37, 38 |
| interpolation | design + tasks | нет | D-03, 20 |
| derive | карточка | нет | D-43 |
| quality | карточки | нет | D-45, 46; proptest; PERFORMANCE.md |

Решения по вариантам — в `design.md` каждой темы.

### Волны

Задача фиксирует контракт первым коммитом: публичные типы с инертными телами и тесты, красные по
assertion; вывод красного прогона — в PR. Второй коммит — поведение. Каждая [P]-задача — своя ветка
и worktree от `Q0`.

```text
Q0 quality-base ─┬─ [P] A listener-delivery ───────┐
                 ├─ [P] B controller-robustness ───┼─► W2 ownership + миграция ─► W3 integration
                 ├─ [P] C motion-clock ────────────┼─► W2 reduce-motion (после B, C)
                 ├─ [P] D physics ─────────────────┼─► W2 retarget (после B, D; A для статусов)
                 ├─ [P] E curves  ├─ [P] F interpolation ├─ [P] G derive
                 └─────────────────────────────────┴─► Z: чистка lib.rs, docs, бенчи «после»
```

| Задача | Разрешённые файлы |
|--------|-------------------|
| Q0 quality-base | `controller.rs` → `controller/{mod,run,tick,status,dispose}.rs` без изменения поведения; `tests/main.rs`; `Cargo.toml` (proptest, `[[test]]`); `benches/*`; тест аллокаций |
| A listener-delivery | `controller/status.rs`, `animation.rs`, `proxy.rs`, `switch.rs`, `compound.rs`, `reverse.rs`, `curved.rs`, `vsync.rs` (сдерживание обхода) |
| B controller-robustness | `controller/{run,dispose,mod}.rs`, `error.rs`, `builder.rs`, `vsync.rs` (attach, дубли) |
| C motion-clock | `controller/tick.rs`, `status.rs`, flui-scheduler `config.rs`/`scheduler.rs`, flui-runtime `frame_clock.rs`/`frame.rs`/`presentation.rs`, flui-platform-api, flui-platform, flui-app `realm_dispatch.rs`, flui-widgets `media_query.rs` |
| D physics | `simulation.rs`, `spring.rs`, `smoothing.rs`, flui-widgets `scroll_physics.rs`, `page_view.rs` |
| E curves | `curve.rs` |
| F interpolation | `tween_types.rs`, flui-foundation `matrix4.rs`/`lerp.rs`, flui-painting `color.rs` |
| G derive | flui-macros `derive_animatable.rs` и его тесты |

Общие файлы: `lib.rs` и `Cargo.toml` крейта — каждая задача добавляет только свои строки,
конфликты разрешает интегратор при rebase; `docs/ARCHITECTURE.md` — задача Z; миграция
`flui-widgets` — W2.

### Бенчи

- Q0 чинит `controller/tick_at` (живой прогон, варианты: без кривой, `EaseInOut`, 1 и 4 слушателя,
  симуляция), `curved_value`, `animated_value_color_frame` (`iter_batched`), `smooth_damp_step`;
  добавляет раздачу статуса N=1/4/8 и стоимость `forward()`.
- «До» — исправленные бенчи на коде `main` (baseline `before`); «после» — на итоговой ветке, тот же хост.
- Аллокации на тик — отдельный `[[test]]` со счётным `#[global_allocator]` (шаблон
  `crates/flui-scheduler/tests/frame_telemetry_allocation.rs`); `cargo xtask globals` не сканирует
  `tests/` и `benches/`.
