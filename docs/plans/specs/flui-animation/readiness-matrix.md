# Animation: трассировка требований к плану

Дата: 2026-10-08. База и результаты — в [evidence](readiness-evidence.md).
Пакеты A0–A9 определены в [плане](readiness-plan.md). Источник ID — исторический
[market.md](market.md); ниже перечислены все 110 требований, без расширения их scope.

Это маршрут проверки, **не таблица 110 закрытых требований**. «Есть» означает найденный
механизм; библиотечный зелёный прогон не доказывает native/render/consumer поведение.
«Вынесено» ссылается на решения в [исторической оркестрации](historical-orchestration.md).
Условный вынос остаётся условным до проверки сценария. Детальные актуальные находки — в
[аудите](readiness-audit.md), старые file:line не используются как новое доказательство.

## Время и прерывания

| Требования | Текущее основание / что ещё доказать | Пакет |
|---|---|---|
| M-TIME-1 | MotionClock есть; общий clock-контракт сверять вместе с production-путями и partition invariance | A3 |
| M-TIME-2 | `one_pump_shares_its_selected_time_across_parent_and_child` проверяет реальный UiRuntime pump: один выбранный timestamp сохраняется для обоих Vsync, даже когда host clock продвигается при выборе времени и внутри callback. Normal/Preserve могут иметь разные локальные timeline согласно motion policy | A3 |
| M-TIME-3 | Отрицательное/убывающее время должно проверяться на входе реестра, не только в чистых часах | A1, A3 |
| M-TIME-4, M-TIME-5, M-TIME-6 | Quiescence/mute частично покрыты; скрытие, resume и owning lifetime требуют end-to-end проверки | A2, A3 |
| M-TIME-7, M-TIME-8, M-TIME-9 | Остались TIME_DILATION и raw f64/testing пути; часы презентации ещё не единый источник | A3 |
| M-TIME-10 | Controller velocity игнорирует кривую; математический retarget не подключён к implicit | A4 |
| M-TIME-11 | Частота анимации/троттлинг вынесены владельцем | Вне текущего scope |
| M-TIME-12 | Спека motion-clock уточняет старую строку: rate ≥ 0, reverse отдельно; синхронизировать формулировки | A3 |
| M-INT-1, M-INT-2, M-INT-5 | Библиотечные сегменты есть; C0/C1 и reversal должны пройти через реальные implicit/controller consumers | A4 |
| M-INT-3 | `changing_only_the_duration_preserves_motion_and_sets_a_new_deadline` проверяет mounted AnimatedOpacity: замена только duration на 400 мс или 2 с сохраняет painted C0/C1, даёт середину независимого кубического эталона и завершает run в новый срок, после которого paint прекращается. `changing_only_the_curve_keeps_the_existing_deadline` отдельно проверяет прежний срок при смене только curve. Duration-сценарий проходит на текущей реализации; отрицательный контроль, ошибочно сохраняющий прежний срок при увеличении duration, нарушает новый midpoint assertion. Это усиление приёмки существующего поведения, не исправление baseline-дефекта | A4 |
| M-INT-4 | Продолжительность и атомарность отказа входят в красные robustness-контракты | A1 |
| M-INT-6 | Dismissible использует max constraints, а не фактический размер; Drawer/back gesture проверить вместе | A6 |
| M-INT-7, M-INT-8, M-INT-9, M-INT-10 | Базовые reverse/seek/cancel/completion тесты проходят; reentrant delivery и lifecycle ещё красные | A1, A4 |
| M-INT-11 | Согласован application-authoring `AnimationController::finish()`, сохраняющий исходный future; ADR-0197, controller contracts и публичный authored-page paint/disposal сценарий | A1 |
| M-INT-12 | Конечность/bounds должны сохраняться при hostile curve/simulation и reentry | A1 |
| M-INT-13 | Условие выноса аддитивной композиции подтверждено: `repeated_interruption_keeps_position_and_velocity_continuous` проверяет 120 прерываний Curve/Spring для scalar/Offset; `opacity_retarget_preserves_the_painted_velocity` и `padding_retarget_preserves_the_laid_out_velocity` проверяют C0/C1 реальных implicit-потребителей. Это не контракт независимых аддитивных слоёв | A4, согласованный вынос |

## Физика и кривые

| Требования | Текущее основание / что ещё доказать | Пакет |
|---|---|---|
| M-PHY-1, M-PHY-2, M-PHY-3, M-PHY-4, M-PHY-5, M-PHY-6, M-PHY-7 | Новые симуляции/валидация и обычные контракты проходят; проверить реальных callers при миграции | A4, A9 |
| M-PHY-8 | DPR scroll tolerance реализован; не считать это автоматически per-type rest threshold | A6, A7 |
| M-PHY-9 | Контракт завершения fling на границе остаётся среди 32 красных | A1 |
| M-PHY-10, M-PHY-11, M-PHY-12, M-PHY-13 | Decay/переход к spring реализованы; scroll consumer suite в этом аудите не запускался | A6, A9 |
| M-PHY-14, M-PHY-15 | Smoothing удалён выбранным решением; не восстанавливать ради старых строк | Удалённый scope |
| M-PHY-16 | Проверить отказ или явное ограничение незавершающихся симуляций на окончательном production-пути | A1, A9 |
| M-CRV-1, M-CRV-2, M-CRV-3, M-CRV-4, M-CRV-5 | Валидация, концы и численные контракты проходят в all-features suite | A8, A9 |
| M-CRV-6 | Steps есть; before-flag исключён уточнённой спецификацией | A7 |
| M-CRV-7 | `PiecewiseLinear` канонизирует `LinearStop`: пропущенные inputs, повышение убывающих inputs, плато и правосторонний внутренний jump. Владелец разрешил сохранить authoring API до появления реальной потребности в штатном consumer. `authored_easing_contract` проверяет численные значения, finite extremes, slope, serde admission и сохранение исходного run при эквивалентном пересоздании; mounted `authored_piecewise_hero_geometry_is_painted_and_retraced` проверяет painted bounds и обратный путь. Эталон stop placement — CSS Easing 2; exterior clamp и точные endpoints следуют принятому FLUI Curve-контракту, raw CSS parsing/extrapolation не заявлены | A7, согласованный authoring scope |
| M-CRV-8 | Экстраполяция кривой вынесена; не смешивать с экстраполяцией интерполируемого значения | Вне текущего scope |
| M-CRV-9 | Владелец согласовал `KeyframesBuilder::cubic()` для авторских траекторий. `authored_cubic_hero_geometry_is_painted_and_retraced` передаёт Keyframes через production-контракт `Hero::create_rect_tween`, сравнивает нарисованную геометрию с независимым эталоном и проверяет обратный переход. Основание сохранения API записано в composition/design | A7, согласованный authoring API |
| M-CRV-10 | `SpringCurve` сэмплирует валидированную аналитическую пружину по progress × conservative rest duration, с overshoot и точными endpoints. Владелец разрешил сохранить authoring API до штатного consumer. `authored_easing_contract` проверяет три режима по независимому ОДУ, duration/refusal, исходный future и value equality; mounted `authored_spring_hero_geometry_is_painted_and_retraced` проверяет реальный overshoot, painted bounds, reversal и unmount. Это duration-based easing, а не velocity-inheriting retarget и не аппроксимационная таблица | A7, согласованный authoring scope |
| M-CRV-11, M-CRV-12 | Input policy проверяется отдельно от параметров. `curve_parameters_reject_invalid_input` сопоставляет независимый ожидаемый CurveError с try_new, panic new и численным serde decode для Cubic, ThreePointCubic, Interval и всех Elastic; включает NaN/±∞ и границы допустимых параметров. Steps проверяет отказ try_new/new и JSON decode в существующих таблицах. Обход проверки x2 или NaN только при decode воспроизводимо нарушает новые случаи. Новые авторские кривые M-CRV-7/M-CRV-10 имеют отдельную приёмку выше | A7, A8 |

## Интерполяция и композиция

| Требования | Текущее основание / что ещё доказать | Пакет |
|---|---|---|
| M-INTP-1, M-INTP-2 | Интерполяция и Oklab реализованы; cross-crate suites этого прохода не запускались | A7, A9 |
| M-INTP-3 | Color хранит u8: premultiply реализован, обещание отсутствия 8-bit stepping отдельно не закрыто | A7, решение scope |
| M-INTP-4 | OkLCh и hue-политики вынесены | Вне текущего scope |
| M-INTP-5, M-INTP-6 | Matrix decomposition/Angle реализованы; проверить render-путь, а не только численные значения | A6, A9 |
| M-INTP-7, M-INTP-8 | TwoWayConverter/derive есть; миграция реальных implicit consumers ещё нужна | A4, A7 |
| M-INTP-9 | ADR-0186: пороги компонентов через TwoWayConverter, derive сохраняет единицы полей; публичные проверки различают geometry/scalar rest, отказ сохраняет run и весь coordinated update. Вектор завершает компоненты совместно, сохраняя premultiplied hue. Нужны итоговый consumer gate и CI | A7, A9 |
| M-INTP-10 | Opt-in discrete values вынесены | Вне текущего scope |
| M-CMP-1, M-CMP-2, M-CMP-4, M-CMP-5 | `cubic_keyframes_pass_through_keys` проверяет смешанные hold/jump/cubic/eased joins по независимым значениям и односторонним производным. `indicator_unmount_mid_frame_releases_controller` меняет signal из реального repaint-источника каждого смонтированного индикатора. Публичный RenderId определяет оставшегося владельца; оба источника получают текущий тик, только оставшийся получает следующий. Reconciliation того же кадра снимает регистрацию и рисует один arc; после полного teardown сохранённые handles не дают уведомлений или paint. Это проверка callback-requested reconciliation, не синхронного unmount внутри обхода реестра | A7 |
| M-CMP-3 | Repeat покрыт обычными тестами; completion/motion-policy проверить после core миграции | A1, A5 |
| M-CMP-6 | Полная WAAPI timing model вынесена | Вне текущего scope |
| M-CMP-7 | Решение удалить Compound ещё требует сверки финального surface: тип остаётся в baseline | A7 |
| M-CMP-8 | Публичные switch value/status reentry и delivery-контракты; `nearby_values_wait_for_their_actual_crossing` проверяет обе стороны crossing. Mounted Navigator `replacement_retains_near_zero_secondary_progress_until_crossing` проверяет реального consumer, включая progress меньше EPSILON. Старая epsilon-логика воспроизводимо нарушает оба сценария | A1, A2 |
| M-CMP-10 | Proxy delivery и reentry сверять отдельно от switch; старый baseline содержал красные контракты | A1, A2 |
| M-CMP-9 | Reverse duration/curve есть; проверить сквозную активность и inherited velocity | A4 |
| M-CMP-11 | Вынос подтверждён для двух сценариев из composition/requirements: циклические фазы через hold+repeat (`indeterminate_bars_follow_the_published_timing`, записанные прямоугольники Material); переходы по состоянию через implicit+retarget (`container_retarget_preserves_the_laid_out_size_velocity`, `container_color_retarget_preserves_painted_alpha_progress`). Произвольный PhaseAnimator не заявлен | A7, согласованный вынос |

## Доступность, владение, интеграция, качество

| Требования | Текущее основание / что ещё доказать | Пакет |
|---|---|---|
| M-A11Y-1, M-A11Y-2, M-A11Y-3, M-A11Y-4, M-A11Y-5 | Сквозная motion policy отсутствует на baseline; PR1515 — зависимость host preferences, не доказательство animation поведения | A5 |
| M-A11Y-6 | Отдельная cross-fade preference вынесена | Вне текущего scope |
| M-OWN-1 | Run ownership/reentry частично покрыты обычными тестами; сохранить при смене owner model | A1, A2 |
| M-OWN-2, M-OWN-6 | Owning lifetime и teardown не закрыты; есть красные контрактные тесты | A2 |
| M-OWN-3, M-OWN-8, M-OWN-9 | Dispose/panic/delivery order воспроизводимо нарушали intended contracts на baseline; сверять конечные ownership и failure-path сценарии | A1 |
| M-OWN-5 | `is_animating_everywhere` проверяет controller/proxy/reverse/curved/tween/switch; `a_switch_tracks_activity_after_hopping_to_a_live_source` проверяет inactive → live hop → completion → reverse → disposal, включая независимый старый source. ConstantAnimation всегда inactive, directional phase сама по себе не означает активный run | A1 |
| M-OWN-4 | Отдельные consumers исправлялись, но полный перенос области/часов не доказан | A2 |
| M-OWN-7 | Listener identity/exhaustion требует проверки конечного реестра, не переноса старых локальных счётчиков | A1 |
| M-INTG-1 | Slide/scale/rotation используют render object; Drawer/Dismissible требуют завершения миграции | A6 |
| M-INTG-2, M-INTG-3 | AnimatedSize/Hero тесты существуют, но не запускались в этом аудите | A6, A9 |
| M-INTG-4 | Удержание exit subtree до completion требует публичного сценария и dispose-проверки | A6 |
| M-INTG-5 | FLIP/layout animation за пределами существующего Hero вынесена | Вне текущего scope |
| M-INTG-6 | Interaction #1514 уже в baseline; сверить единый gesture→fling consumer path с его фактическим контрактом | A6 |
| M-INTG-7 | Платформенные spline/deceleration модели вынесены | Вне текущего scope |
| M-INTG-8 | Без scope implicit может молча не тикать; owning presentation fallback должен быть явным | A2 |
| M-INTG-9 | Debug slow-motion ещё не связан с едиными часами презентации | A3 |
| M-INTG-10 | Новый настраиваемый hit-test surface вынесен; корректность существующих hooks обязательна | A6, без расширения surface |
| M-QLT-1, M-QLT-2 | Обычные numerical/property tests проходят; это не закрывает 32 ignored contracts | A8, A9 |
| M-QLT-3 | Чистая clock math не доказывает partition invariance всей цепочки runtime→widget | A3 |
| M-QLT-4 | `bench_live_tick` сохраняет живой run; mounted_motion предварительно проверяет 96 движущихся кадров, submitted scene и изменение progress, затем настоящий idle. Активная работа, admission retarget и idle измеряются отдельно. Парное CPU-измерение одинаковых fixtures выполнено; результаты ограничены конфигурацией и измеренными revision, не являются бюджетом GPU/frame latency | A8 |
| M-QLT-5 | `a_steady_state_frame_allocates_nothing` измеряет allocation calls и bytes на 10 000 продвигающихся tick для scalar, geometry/color/keyframes и relay depth 1/5/32, с положительным контролем и проверкой прогресса. `retiring_idle_registry_owners_allocates_nothing` проверяет retirement отдельно. Полный mounted frame не объявлен безаллокирующим | A8 |
| M-QLT-6 | README/GUIDE/PERFORMANCE сверены с актуальными публичными контрактами: Catmull-Rom только между последовательными cubic, соседняя eased-скорость и нулевая hold/jump-скорость; стоимость notifier-backed регистрации отделена от ConstantAnimation. GUIDE и rustdoc Stagger используют saturating subtraction для one-shot и безопасную модульную фазу для цикла. Этот ограниченный аудит трёх документов не объявляет проверенной всю документацию workspace | A8 |
| M-QLT-7 | `cargo xtask check-changed --base 9e433d6a6c8f1dd4cbfae2038593717ed0eb6c18` прошёл на финальной версии исходников и README/GUIDE/PERFORMANCE: strict rustdoc, doctests через include_str, 676 тестов и обе feature-матрицы по 34 комбинации. 19 тестов пропущены по конфигурации; это не утверждение о запуске каждого документа или платформы workspace | A8, A9 |
| M-QLT-8 | animation_it существует; окончательную раскладку public/private tests проверить при миграции | A8 |
| M-QLT-9 | `ConstantAnimation::with_status` согласован владельцем как авторский API для неизменного значения с выбранной фазой; `a_constant_phase_does_not_install_a_run` проверяет отсутствие запуска во всех фазах и через wrappers. Назначение записано в controller-robustness/design. MotionSpec/AnimatedValue и часть surface ещё требуют production callers либо явного удаления | A4, A7 |
| M-QLT-10 | Проверка process markers — репозиторный gate, не ручное обещание | A8 |
| M-QLT-11 | В этом проходе выполнен all-features, включая serde; сохранить в финальном CI | A8, A9 |

## Маршрут исторических дефектов

Каждый D-ID присутствует ровно один раз. Это назначение владельца повторной проверки,
а не заявление, что все 47 дефектов остались. Например, старый fixed 1/300 заменён,
но actual-size сценарий всё ещё не закрыт; smoothing удалён целиком.

| Пакет / disposition | Исторические ID |
|---|---|
| A1 delivery и robustness | D-01, D-02, D-04, D-06, D-07, D-09, D-10, D-11, D-14, D-26, D-27, D-31, D-32, D-34, D-35, D-44, D-47 |
| A2 owning lifetime | D-21, D-22, D-28, D-29, D-42 |
| A3 clocks | D-25, D-33 |
| A4 retarget | D-08, D-36 |
| A5 reduce motion | D-05 |
| A6 rendering/gesture consumers | D-13, D-15, D-16, D-30, D-41 |
| A7 surface/composition | D-43 |
| A8/A9 сохранить и проверить merged numerical fixes | D-03, D-12, D-17, D-18, D-19, D-20, D-23, D-37, D-38, D-39, D-40 |
| A8 docs/bench/test layout | D-45, D-46 |
| Удалённый smoothing | D-24 |
