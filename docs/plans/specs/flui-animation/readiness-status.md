# Animation: актуальные контракты и границы приёмки

Источник — 110 строк [market.md](market.md); исторический маршрут сохранён в
[readiness-matrix.md](readiness-matrix.md). Здесь каждой строке назначен текущий
контракт, наблюдаемое доказательство либо согласованное исключение. Это не
обещание выполнения каждой платформы и каждого workload на текущем компьютере.

«Сценарий» означает исполняемый контракт текущего production-пути. «Авторский»
означает явно сохранённое владельцем расширение с публичной приёмкой.
«Вынос» ссылается на scope [readiness-plan.md](readiness-plan.md), уточнённый
тематическими requirements. «Внешняя проверка» сохраняет указанную границу
native/GPU/измерений и не превращает старый результат в current-head receipt.

Имена тестов разрешаются командой `rg -n '<имя>' crates packages`.
Численные проверки находятся в `crates/flui-animation/tests/contracts`, mounted
сценарии — в `flui-widgets/tests`, пакетах и runtime. Терминальные счётчики и
внутренний presentation gate требуют документированного private seam; это
дополнение к публичным owner/frame сценариям, а не их замена.

## Время

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

## Прерывания

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

## Физика

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

## Кривые

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

## Интерполяция

| ID | Disposition | Контракт и доказательство |
|---|---|---|
| M-INTP-1 | Сценарий | Generic `Tween<T: Lerp>`; `integer_tweens_interpolate_across_the_full_range` задаёт independent round/floor extrema, NaN и clamps |
| M-INTP-2 | Сценарий | Premultiplied Oklab numeric references и typed color motion сохраняют hue при fade, geometry/color идут через реальные implicit callers |
| M-INTP-3 | Внешняя проверка | Continuous Color и engine paint/filter маршруты (ADR-0194); existing GPU readback различает fractional alpha в Tween/group/DecorationImage. Новый current-head GPU run здесь не заявлен |
| M-INTP-4 | Вынос | OkLCh/hue policies исключены владельцем |
| M-INTP-5 | Сценарий | Interpolation I1–I6/I14 уточняют collapsed scale, borrowed rotation, skew/perspective и nondecomposable discrete fallback; foundation oracle и mounted AnimatedContainer |
| M-INTP-6 | Сценарий | `animated_rotation_takes_the_requested_arc` различает Numeric/Shorter и многооборотную траекторию по реальному transform |
| M-INTP-7 | Сценарий | Fixed AnimationVector + owning AnimatedValue, vector sample commit; PropertyMotion и coordinated updates не требуют per-component owners |
| M-INTP-8 | Сценарий | `two_way_converter_derive_contract` использует nested external DTO и настоящий retarget; SDK nested_motion fixture импортирует trait+derive через prelude |
| M-INTP-9 | Сценарий | Component thresholds сохраняют единицы; geometry/scalar заканчивают в разные сроки, invalid thresholds не меняют оба owners и их futures (ADR-0186) |
| M-INTP-10 | Вынос | Discrete opt-in surface исключён владельцем |

## Композиция

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

## Доступность

| ID | Disposition | Контракт и доказательство |
|---|---|---|
| M-A11Y-1 | Внешняя проверка | Public host→runtime→MediaQuery/FrameTick проверяет first/live preference и wake-before-settle; physical platform preference producer требует отдельного native receipt |
| M-A11Y-2 | Сценарий | FollowSystem/Reduce/Full override имеет девять комбинаций и first-root/public runtime tests, preference принадлежит презентации |
| M-A11Y-3 | Сценарий | Essential scroll/refresh Preserve, indicators Normal; reduced mounted fixtures различают motion и static feedback |
| M-A11Y-4 | Сценарий | Finite oneshot/repeat settle в declared endpoint, infinite repeat parks pending; Full/rebind возобновляет, completion/status не пропадают |
| M-A11Y-5 | Внешняя проверка | Validated host duration scale и public runtime updates; zero scale settles next frame, invalid input refuses, tiny scale saturates. Native backend execution имеет отдельную границу |
| M-A11Y-6 | Вынос | Отдельный cross-fade preference исключён владельцем |

## Владение

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

## Интеграция

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

## Качество

| ID | Disposition | Контракт и доказательство |
|---|---|---|
| M-QLT-1 | Сценарий | Public curve catalog/properties + independent references: endpoint, admitted output/overshoot и monotonic families; constructor admission отдельно от input progress |
| M-QLT-2 | Сценарий | Independent spring regime and friction/crossing oracles; smoothing удалён, его старые properties не считаются доказательством нового поведения |
| M-QLT-3 | Сценарий | Mounted partition + pure curve/spring/repeat properties; frame count не подменяет equal output/outcome assertions |
| M-QLT-4 | Внешняя проверка | Published benches удерживают live run, проверяют 96 движущихся frames/scene/progress и отдельный idle; исторические paired CPU timings ограничены revisions/workloads. Новый speedup/GPU budget не заявлен |
| M-QLT-5 | Сценарий | `a_steady_state_frame_allocates_nothing`: 10 000 advancing warmed scalar/vector/keyframes/relay ticks, calls+bytes и positive control; idle retirement отдельно. Whole mounted frame не объявлен безаллокирующим |
| M-QLT-6 | Сценарий | README/GUIDE/PERFORMANCE и crate architecture обновлены: canonical imports, ownership, curve/physical spring distinction, retained authoring и точные границы старых измерений |
| M-QLT-7 | Сценарий | Crate prose включено через include_str doctests; scope gate выполняет private rustdoc/doctests и compile examples. Ignored blocks/platform docs не выдаются за исполненные |
| M-QLT-8 | Сценарий | Один animation_it binary + общий table runner; allocation/child-process/terminal private seams имеют отдельную обоснованную topology |
| M-QLT-9 | Сценарий | Canonical root/prelude и consumer disposition в crate architecture; aliases/unwired pure adapters удалены, actual callers мигрированы. Builder/finish/chosen-phase/cubic/authored easing сохраняют explicit owner-approved authoring scope |
| M-QLT-10 | Сценарий | Repository markers/checks gate выполняется; requirement/process IDs остаются только в approved specs/history |
| M-QLT-11 | Сценарий | Numeric construction/serde table и authored easing исполняются с all-features; compilation feature alone не заменяет decode assertions |

## Что доказательство не расширяет

Текущие local checks не являются current-head native smoke, GPU readback или
повторным performance measurement. Эти границы сохранены в соответствующих
строках. Public API cleanup намеренно несовместим с прежними module/alias paths;
SemVer assessment и инструкции migration относятся к этому изменению, а не к
восстановлению параллельного legacy API.

Смена только duration у implicit curve дополнительно проверяется
`changing_only_the_duration_preserves_motion_and_sets_a_new_deadline`, а смена
только curve — `changing_only_the_curve_keeps_the_existing_deadline`. Эти случаи
не подменяют физический velocity-inheritance контракт M-INT-3.
