# flui-interaction — закрытие строк матрицы

- **Дата:** 2026-10-06. Источник строк — [matrix.md](matrix.md); решения — [orchestration.md](orchestration.md) («Scope-решения»).
- **Правило:** каждая строка не в статусе present закрывается одним способом: draft-PR этой работы, задача утверждённой спеки, новая задача (NEW) или исключение (OUT).
- **ID строк:** с префиксом раздела (`M1-14`, `M2-F1`, `M3-F1`), потому что `A*`, `D*`, `F*`, `R*` повторяются в M2 и M3.
- **Сертификация (release/requirements.md):** нативно сертифицируется только Windows; Linux — через CI; macOS, Web, Android, iOS — `experimental`. Задачи P3 для них остаются в спеке pointer-vocabulary, но проверяются только `cross-typecheck`/`wasm-check`.

## Исходная разметка

Числа ниже относятся к разметке 2026-10-06, а не к текущему числу открытых строк.
Состояние реализации повторно сверено 2026-10-07; текущие зависимости основной
работы записаны в [tasks.md](tasks.md), а завершённые NEW-пункты отмечены ниже.
Сверка исходников и merged-PR не заменяет повторного прогона тестов.

| Способ закрытия | Строк |
|---|---|
| PR (draft этой работы) | 16 |
| Спека (утверждённая задача) | 53 |
| NEW (15 предложенных задач) | 20 |
| OUT | 6 |
| **Всего не-present** | **95** |

Разбивка по спекам: pointer-vocabulary 23, flui-interaction (I10/I11/S2/S3/S5) 14, send-flip T6d 8, focus-keyboard 6, text-ime 1, platform-layer 1.

PR #1467 (held Down на blur), #1476 (контрактные тесты hover/hit-test), #1478 (типы словаря) и #1482 (docs) сами строк не закрывают: #1467 — runtime-шов I7 (строка M1-15 уже present), #1476 — тесты для T6d (C5, H13–H16), #1478 — P1, на котором стоят задачи P2/P3, #1482 — документация (часть S3).

## PR

Эта таблица сохраняет исходное назначение строк PR на 2026-10-06.
Колонка baseline не описывает текущую реализацию: её проверки и ограничения
записаны в [tasks.md](tasks.md) и в текущих строках ниже. 40 ms debounce
двойного тапа теперь подтверждён публичной boundary-проверкой и независимым
откатом; это отдельное доказательство от наличия исторического merged-PR.

| Row | Requirement | Baseline status (2026-10-06) | Assigned closure |
|---|---|---|---|
| M1-14 | OS-level capture (drag leaves the window) | broken (Win32, web) | PR #1471 (Win32 `SetCapture`/`WM_CAPTURECHANGED`; web — pointer-vocabulary V14) |
| M2-T3 | Button filtering for non-tap recognizers | broken | PR #1474 |
| M2-T4 | Double tap: timeout, slop between taps, debounce | partial | Existing timeout/per-kind slop are retained; debounce is now implemented locally, with local gate passed; CI/merge pending. Second Down is eligible at 40 ms after first Up on the frozen owner-clock snapshot. Public Mouse/Touch 39/40 ms, held-first-contact and reused-ID recovery rows pass. Independent debounce inverse admits the 39 ms Down and fails, exact40 control stays GREEN; exact source was restored and broader public/trybuild plus cancellation-reuse checks passed |
| M2-L1 | Long press timeout and movement tolerance | partial | PR #1474 |
| M2-X3 | Per-device-kind settings | partial | PR #1474 (`touch_slop()` crate-private — S1) |
| M2-V3 | Velocity samples use the event timestamp | broken | PR #1474 |
| M2-T5 | N-tap / consecutive-tap count | absent | PR #1472 (TapAndDrag consecutive clicks) |
| M2-S2 | Rebaseline on pointer add/remove during scale | broken | PR #1472 |
| M2-S3 | Degenerate span safety | broken | PR #1472 |
| M2-S4 | Rotation unwrap across ±π, stable ordering | broken | PR #1472 |
| M2-S5 | Scale callbacks contained; arena per pointer | partial | PR #1472 |
| M2-F1 | Force press (sensor-less 0.5 on web) | partial | PR #1472 |
| M2-V5 | Stop detection on the samples' clock | broken | PR #1479 (`estimate_at`; wiring in recognizers — C5) |
| M2-V6 | Min/max fling clamp from settings | partial | PR #1479 |
| M2-V7 | NaN / non-finite safety in velocity | partial | PR #1479 |
| M2-R1 | Resampling to vsync (event time, no lost Up) | partial | PR #1479 |

## Спека

Историческая приёмка до публикации PR #1514: owned vocabulary и прямые producers
в строках ниже интегрированы, но producer
verification pending означает ограничения из `pointer-vocabulary/tasks.md`:
предыдущая Apple cross-typecheck и свежая строгая Android libraries проверка —
только компиляция, pen/touch activation
на Windows отказал; fractional hidden-HWND wheel smoke прошёл. Локальный
`check-changed` на `5f28646ad` прошёл; CI/merge ещё впереди. Финальные native
macOS/iOS/Android и Linux execution пропущены по отсутствующим инструментам
и host capabilities; ранняя cross-typecheck означает только компиляцию.
Строки внешних focus-keyboard/text-ime/platform-layer спецификаций этим не закрываются.
Текущие I11 gesture/wheel строки обновлены отдельно по
[локальной приёмке](tasks.md#текущая-локальная-приёмка-i11-gesturewheel): реализация
и целевые проверки завершены; заключительный gate, all-features clippy/compile-fail,
Win32 wheel smoke и CI ещё ожидаются. Это не закрывает LY8/LY9 целиком.

| Row | Requirement | Status | Closure |
|---|---|---|---|
| M1-1 | Device kinds mouse/touch/pen | implemented locally; producer verification pending | spec pointer-vocabulary P3-Win32 (V9); unified enum — P2 V6 |
| M1-2 | Eraser / inverted stylus | implemented locally; producer verification pending | spec pointer-vocabulary P3-Win32 (V9) |
| M1-3 | Pressure (real sensor) | implemented locally; producer verification pending | spec pointer-vocabulary P3-Win32 (V9) |
| M1-5 | Tilt / altitude-azimuth | implemented locally; producer verification pending | spec pointer-vocabulary P3-Win32 (V9); iOS — V12 |
| M1-6 | Twist (barrel rotation) | implemented locally; producer verification pending | spec pointer-vocabulary P3-Win32 (V9) |
| M1-7 | Contact width/height | implemented locally; producer verification pending | spec pointer-vocabulary P3-Android (V13); typed `ContactSize` — PR #1478 |
| M1-8 | Pen hover | implemented locally; producer verification pending | spec pointer-vocabulary P3-Win32 (V9) |
| M1-16 | pointercancel: system gesture / OS takeover | implemented locally; producer verification pending | spec pointer-vocabulary P3-web (V14) |
| M1-17 | pointercancel: device removed | implemented locally; producer verification pending | spec pointer-vocabulary P3-Win32 (V9) |
| M1-18 | Multi-touch | implemented locally; producer verification pending | spec pointer-vocabulary P3-Win32 (V9) |
| M1-19 | Primary pointer | implemented locally; producer verification pending | spec pointer-vocabulary P2 (V6) |
| M1-21 | Chorded buttons / button change | implemented locally; producer verification pending | spec pointer-vocabulary P2 (V6) (`ButtonChange`); Win32 producer V9 |
| M1-22 | Device id (persistent) | implemented locally; producer verification pending | spec pointer-vocabulary P2 (V6) |
| M1-23 | Timestamps: monotonic, platform-provided | implemented locally; producer verification pending | spec pointer-vocabulary P3-Win32 (V9) (`GetMessageTime`, deferred from #1471) |
| M1-26 | Wheel delta modes line/pixel/page | implemented locally; producer verification pending | spec pointer-vocabulary P2 (V7) (resolution in `Scrollable`, step from LY8 `wheel()`) |
| M1-29 | Scroll / gesture phases | implemented locally; producer verification pending | spec pointer-vocabulary P3-macOS (V11) / winit V10 |
| M1-30 | Momentum phase + inertia cancel | implemented locally; producer verification pending | spec pointer-vocabulary P3-macOS (V11) |
| M1-31 | Trackpad pinch / rotate as distinct events | implemented locally; producer verification pending | spec pointer-vocabulary P3-winit (V10) |
| M1-32 | Trackpad two-finger pan delivered through native scroll | implemented locally; producer verification pending | AppKit `scrollWheel:` produces one `ScrollEvent` with phase/momentum and precision; magnify/rotate share a separate cumulative `PanZoom` stream. Approved pointer-vocabulary P3-macOS (V11) and ADR-0143 preserve this distinction without duplicate scroll |
| M1-36 | High-DPI: logical vs device px types | implemented locally; producer verification pending | spec pointer-vocabulary P2 (V6); web float coords — V14 |
| M1-39 | Android mouse wheel (`ACTION_SCROLL`) | implemented locally; producer verification pending | spec pointer-vocabulary P3-Android (V13) |
| M1-40 | Win32 pen and touch (`WM_POINTER`) | implemented locally; producer verification pending | spec pointer-vocabulary P3-Win32 (V9) |
| M3-D1 | Logical vs device px at input ingress | implemented locally; producer verification pending | spec pointer-vocabulary P2 (V6) |
| M1-20 | Buttons bitmask incl. X1/X2 | partial | spec focus-keyboard T5 |
| M3-F7 | Focus restore after focused node removed | absent | spec focus-keyboard T7 (R15) |
| M3-F10 | Focus visible (input modality) | absent | spec focus-keyboard T3 (R17) |
| M3-F11 | Focused element scrolls into view | absent | spec focus-keyboard T11 (R6; list only) |
| M3-K4 | Pressed-key set sync on window focus change | absent | spec focus-keyboard T6 |
| M3-K8 | Full default intent set | partial | spec focus-keyboard T8 (arrows after M3-F4) |
| M3-K6 | IME composition flag on key events | partial | spec text-ime T5 |
| M1-9 | Coalesced events kept | implemented locally; local gate passed; CI/merge pending | Binding preserves three packets' six measured readings and only the newest prediction family. All eight slop-sensitive recognizers inspect delivered measured excursions, including origin-return packets, without prediction admission or per-sample callback replay. Public queued/authored history, reentry and recovery rows pass; eight independent admission inverses fail their isolated rows and exact source hunks were restored |
| M2-V4 | No sample loss to the velocity tracker | implemented locally; local gate passed; CI/merge pending | Drag, multi-drag, scale and tap-and-drag feed delivered measured history with production timestamps into the selected estimator; bounded estimator retention remains deliberate. `resampler_interpolates_on_event_time_and_never_drops_terminals` pins merged measured history and newest predictions; selected-estimator and terminal-clock families pin the consumers |
| M3-H3 | Non-finite positions rejected at the edge | absent | spec send-flip T6d (handoff H17) |
| M3-H10 | Dead or legacy surface | broken (unwired) | spec send-flip T6d (handoff N6) |
| M3-C5 | Cursor defer vs explicit arrow | broken | spec send-flip T6d (contract test in PR #1476) |
| M3-F2 | Reading order with row bands, RTL-aware | broken | spec send-flip T6d (I6 handoff) |
| M3-F6 | Modal focus trap | partial / broken | spec send-flip T6d (N6: wire or delete `traps_focus`) |
| M3-R1 | RTL in focus traversal | absent | spec send-flip T6d (I6 handoff, with M3-F2) |
| M2-X1 | System timings from the OS | implemented locally; final acceptance checks pending | ADR-0172: host-owned `SystemPreferences` publishes timings through the existing `GestureArenaScope`; `admitted_gesture_settings_contract` and `owner_metrics_contract` check admission snapshots and ordered updates. Broader LY8/LY9 acceptance remains separate |
| M1-10 | Predicted events | withdrawn (local extrapolator) | S2: неподключённый `InputPredictor` удалён; аппаратные predicted samples сохраняются в owned-событиях и локализации по pointer-vocabulary P2/P3. Синтез будущих samples не поддерживается |
| M1-24 | Click count / interval from the OS | observed interval implemented locally; count remains recognizer-owned | I11 / ADR-0172: `native_mouse_interval_starts_at_first_down` pins observed mouse press-to-press timing; touch and authored timing retain their release-to-press policy. Consecutive candidates retain the admitted timing origin |
| M2-A5 | Arena teams | withdrawn | S2: неподключённые team/TeamEntry и multiple-winner resolution удалены. Typed exclusive/require-first-failure composition реализована в отдельной NEW-строке M2-A6; она не возвращает удалённую team surface или multiple-winner contract |
| M2-A7 | Pointer-signal arbitration | withdrawn (standalone priority resolver) | S2: отдельный неподключённый resolver удалён. Действующая арбитрация widget hit-path сохраняется: `a_wheel_tick_over_nested_scrollables_moves_only_the_inner` проверяет ближайшего claimant и отсутствие двойного scroll |
| M2-T6 | Multi-finger tap semantics | partial | spec flui-interaction S3 (record as a mapping decision) |
| M2-D2 | Pan slop value | implemented locally; final acceptance checks pending | I11 / ADR-0172: native touch projection preserves the authored pan-to-hit and per-axis ratios; `native_touch_span_preserves_baseline_ratio` and `native_touch_projection_preserves_authored_tier_ratios` pin this policy |
| M2-D6 | Mouse drag threshold from the OS | implemented locally; native limits explicit | I11 / ADR-0172: per-presentation geometry projects native mouse rectangles into logical coordinates; `mouse_rectangle_is_compared_per_axis` and `native_geometry_controls_admission_and_retries_without_a_frame` cover admission and recovery. Windows query executes; Android/AppKit evidence is Rust-only compilation |
| M2-V2 | Estimator choice reaches production | implemented locally; uniform framework baseline selected | `GestureSettings::with_velocity_estimator` reaches drag/multidrag/scale/tap-and-drag; selected-estimator rows in `gesture_lifecycle_matrix` pass. ADR-0172 keeps least squares for absolute pointer gestures across platforms. Estimator selection is consumer policy, not an OS preference. `native_scale_retains_estimator_until_end_and_readmits_next_begin` and `mounted_native_begin_retains_estimator_before_first_claim` pin native admission |
| M2-R2 | Prediction | withdrawn (local extrapolation) | S2: локальный extrapolator удалён; сохранение аппаратных predicted samples не означает реализацию синтезированной prediction для ink/drag |
| M2-X2 | Settings profile reaches recognizers | authored and live host projections implemented locally | I11 / ADR-0172: the existing `GestureArenaScope` supplies mounted production builders; contacts, native sessions and consecutive candidates freeze admission settings. `admitted_gesture_settings_contract`, `pointer_and_gesture_recognition` and `owner_metrics_contract` cover live publication, overrides and recovery; final acceptance checks remain pending |
| M2-X5 | Cheapest sound ownership on the gesture path | owner-local implementation integrated; original local gate passed | I10/recognizer-api use Rc/RefCell, weak arena slots and immutable configuration. I11 adds a read-only owner-local live projection; active admission snapshots remain immutable. Current final gate and CI are separate pending evidence |
| M3-C1 | Re-hit-test after layout per presentation | partial | spec flui-interaction S5 |
| M3-D2 | DPI change per presentation | partial | spec flui-interaction S5 |
| M3-M1 | Per-window input state | partial | spec flui-interaction S5 |

## NEW

Историческая сверка до публикации PR #1514, 2026-10-08 на интеграционной базе `5f28646ad`: все 20 утверждённых строк
ниже имеют реализацию или ранее merged-реализацию. Это не означает завершение
15 задач приёмки целиком: внешний I11, CI/merge и native hardware ограничения
остаются явными условиями. Локальный final gate прошёл. Целевые прогоны прошли для 43 pointer-строк,
56 scroll-строк, нижнего reveal/retirement и counting-allocator контракта.
Повторный restored-прогон на базе `9d171de1` проверил binding,
private resampling, lower reveal/retirement и обе публичные widget-семьи.
Counting-allocator matrix проверена отдельно, а не через фильтр по
pointer/binding именам тестов. Точный identity и global route с обеими history
дают ноль allocations; translated и near-identity route сохраняют bound
2/8/32 для 1/4/16 targets. Sample/Stop без изменения времени дают ноль,
а raised-floor control сохраняет checked history policy. Saturated admission
сохраняет cap 100 с передачей owned history storage: не более одной allocation,
199 сохранённых readings в 99 Move, metadata, predictions и Down/Up проверены.
Независимые identity/unchanged-time inverses воспроизводят две лишние копии;
consumer-only overflow inverse дал семь allocations при зелёной API table.
Последующий transfer case был RED при трёх allocations против budget одной;
producer-only inverse даёт две при зелёной новой API table. Десять целевых
API/resampling/allocator/binding тестов прошли; runtime containment этим
прогоном не выбран. Точный inverse source-хунк восстановлен без diff.
Все хунки восстановлены, целевые публичные проверки прошли. Подробности —
в `tasks.md`, «Публичные allocation contracts».
Локальный `cargo xtask check-changed --base d6ad274194483c6d1bc100f9a14d42e3890b6c0e`
на `5f28646ad` завершился exit 0: driver 46/46, workspace 793/793 (62 skipped),
strict clippy/rustdoc/doctests, Windows required-feature all-targets и wasm
checks, ordinary platform trybuild прошли. Native macOS/iOS/Android и Linux
execution пропущены; отдельная cargo-hack feature matrix не запускалась.
CI ещё не опубликован, слияние в `main` не выполнено; детали в `tasks.md`.
После пяти независимых CombinedMode/scale-velocity/focal-fling/rotation/boundary
откатов точные production-хунки восстановлены; повторный прогон всех 43
pointer/gesture widget-строк на базе `85a5f10e6` прошёл. Это не заменяет
непроверенные native/hardware пути и CI. Interaction source doctests и восемь
прямых Markdown-примеров прошли; GestureDetector snippet проверен widget
doctests (33 runtime и три compile-fail; 13 других примеров ignored).
Все пять wire-бенчей прошли окончательное измерение на logical CPU 0 при
normal priority: 33 полные пары и шесть AFTER-only cases, production AFTER
`3d6fae0de`. PERFORMANCE.md сохраняет means, 95% confidence intervals,
historical adapters, source revisions и объяснения изменённых workloads.
Промежуточные borrowed-transform means показывают elapsed-time regressions
+8.39% для четырёх targets и +4.68% для 16 targets; финальные девять route
shapes заменены полной парной проверкой, прежние наблюдения не выдаются за
окончательный результат. Отдельная свежая пара пяти estimate cases на
`c1dc17881` проверила private kernel isolation: LSQ20 -1.52%, LSQ3 +0.53%,
четыре queries +8.24%, Impulse -0.05%, Ios +237.51%; исходный LSQ20 regression
сохранён в PERFORMANCE.md. Ios сохраняет исправленный eligible history window.
Restored-прогон одиннадцати публичных тестов включает настоящий flui-testing
runtime containment; он не смешивается с более ранними десятью тестами без
containment. Timing acceptance и локальный gate прошли; CI и слияние pending.

| Row | Requirement | Status | Closure |
|---|---|---|---|
| M1-11 | Resampling to the vsync (enable policy) | implemented locally; local gate passed; CI/merge pending | `PresentationConfig::with_pointer_resampling` connects frame-aligned delivery; public pointer and private `resampling_is_presentation_local_and_preserves_delivery_after_failure` families pass. Independent arrival/frame inverses fail in both paths; pending-wake inverse fails the redraw obligation; restored private family passes |
| M1-13 | Explicit pointer capture + lost capture | implemented locally; local gate passed; CI/merge pending | `PointerCapture` preserves the Down route and defers one `CaptureLost`; `explicit_pointer_capture_contract` passes, four independent production inverses fail and restored sources pass; ADR-0164 |
| M3-H8 | Explicit pointer capture/release API | implemented locally; local gate passed; CI/merge pending | Same token contract, mounted `listener_capture_retains_one_target_and_drop_delivers_loss`; native capture remains a separately checked producer contract |
| M1-27 | High-precision vs notched wheel | implemented locally; local gate passed; CI/merge pending | Owned `ScrollPrecision` reaches `Scrollable`; only notched deltas animate. Current public families and actual fractional hidden-HWND producer smoke pass. Precision-only native inverse reports Unknown instead of required Precise and fails; exact restored producer smoke passes. This does not establish pen/touch activation |
| M1-28 | Smooth notched-wheel scrolling | implemented locally; local gate passed; CI/merge pending | `notched_wheel_accumulates_distance_and_eases_out_in_150ms`, interruption/replacement/unmount/sibling rows pass; zero-duration inverse fails four actual scroll rows, restored smooth-wheel rows pass |
| M1-34 | Shift+wheel → horizontal scroll | implemented | PR #1487 merged: `wheel_axis_delta`, `shift_wheel_scrolls_the_horizontal_axis` |
| M1-35 | Scroll latching | implemented locally; local gate passed; CI/merge pending | `nested_scroll_sequence_keeps_its_first_consumptive_target`, phase/cancel/timeout/device/source-local rows pass. Independent focus-drain, device-removal and exact kind/role identity inverses fail accepted delivery/count assertions; restored binding families pass |
| M2-A6 | Competing-recognizer composition | implemented locally; local gate passed; CI/merge pending | `GestureCompetition` feeds real GestureDetector arbitration; eight arena rows pass, admission inverse fails and restored sources pass |
| M2-D5 | Multi-pointer drag strategy | implemented locally; local gate passed; CI/merge pending | `DragPointerStrategy::ContinueWithRemaining` reaches GestureDetector/Scrollable; continuation and reentrant cancellation rows pass. Reverting continuation produces premature Scrollable fling and ends the recognizer on the first Up instead of retaining the remaining contact |
| M2-S1 | Scale + rotate consumed by a widget | implemented locally; local gate passed; CI/merge pending | Scale recognizer reaches mounted InteractiveViewer; pivot rotation, finite recovery, touch transition and focal-fling rows pass. Combined-mode inverse fails three actual contracts; zero-impulse focal-fling inverse fails both progress and rebuild/geometry rows; rotation-only inverse fails pivot and recovery; MAX finite-boundary inverse loses the boundary result. Native lease inverse changes the mounted Viewer scale from the required 1.5 to 1.2 and fails ten binding rows; each hunk was restored |
| M2-S6 | Scale end velocity | implemented locally; local gate passed; CI/merge pending | `viewer_reports_scale_velocity_separately_from_focal_velocity` and terminal event-clock cases pass; scalar-scale-velocity inverse fails the distinct-units contract and exact production hunk was restored |
| M2-S7 | Trackpad pan/zoom fed to recognizers | implemented locally; local gate passed; CI/merge pending | Native claim/session owner connects PanZoom to Scale/Viewer; repeated Start, descendant rebuild and terminal ownership rows pass. Independent lease, focus-drain, device-removal and exact kind/role identity inverses fail; restored binding families pass |
| M2-X4 | Nested scroll fling handoff | implemented locally; local gate passed; CI/merge pending | All 56 scroll rows pass, including the actual receiver DPR=2 case. Independent delivery, bounce-parent policy, equal-edge reentrant jump, ordinary same-controller rebuild, custom-physics first-failure ordering and DPR inverses fail; exact restored sources pass the full family. ADR-0169 |
| M3-H2 | Perspective transforms unproject the ray | implemented locally; local gate passed; CI/merge pending | Two public transform matrices pass; position and vector/widget inverses fail and restored sources pass; ADR-0162 |
| M3-F3 | Explicit traversal order and groups | implemented locally; local gate passed; CI/merge pending | Group/weak override production path is covered by 28 mounted focus rows and public/private containment; ADR-0165 |
| M3-F5 | Scope edge behaviour | implemented locally; local gate passed; CI/merge pending | Widget scope edges and nested actual group/scope retries pass; policy order is reused during a parent retry |
| M3-F4 | Directional navigation | implemented locally; local gate passed; CI/merge pending | Four-way beam/gap/distance search, arrow fallback and reentrant geometry pass. Provider containment inverse fails and restored sources pass; broader traversal inverses remain separate |
| M3-K5 | Dead keys | implemented | PR #1489 merged: Win32 emits `Key::Dead`; production conversion contract rows |
| M3-K9 | Character shortcuts independent of Shift | implemented | PR #1490 merged: `SingleActivator::character(...).ignoring_shift()` and shortcut contracts; a separate duplicate type is unnecessary |
| M3-A5 | Scroll actions and ShowOnScreen on scrollables | implemented locally; local gate passed; CI/merge pending | Existing axis actions plus routed ShowOnScreen cover both axes/reverse, nested ancestors, published geometry and sibling reentry. All 56 scroll rows and lower reveal/retirement pass; published-basis and retirement inverses fail and restored sources pass |

## OUT

| Row | Requirement | Status | Closure |
|---|---|---|---|
| M1-4 | Tangential pressure | partial | OUT: the field exists; Win32 has no barrel wheel, web is experimental |
| M2-A11 | `cancelsTouchesInView` equivalent | absent | OUT: low priority, only needed for platform-view embedding, which 0.2 doesn't ship |
| M2-L2 | Post-accept slop for long press | partial | OUT: optional, no consumer; the default is no post-accept cancel |
| M2-V8 | 1D velocity tracker | absent | OUT: the projected 2D tracker is acceptable |
| M2-R3 | Smoothing (1€ filter) | partial | OUT: fine as a standalone utility, kept off the gesture path |
| M3-M3 | Drag-and-drop input routed | absent | OUT: system DnD is unimplemented per ADR-0038 and not in the 0.2 release scope |
