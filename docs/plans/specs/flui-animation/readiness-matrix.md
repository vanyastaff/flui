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
| M-TIME-1, M-TIME-2 | MotionClock есть; единое типизированное время всех production-путей ещё не проведено | A3 |
| M-TIME-3 | Отрицательное/убывающее время должно проверяться на входе реестра, не только в чистых часах | A1, A3 |
| M-TIME-4, M-TIME-5, M-TIME-6 | Quiescence/mute частично покрыты; скрытие, resume и owning lifetime требуют end-to-end проверки | A2, A3 |
| M-TIME-7, M-TIME-8, M-TIME-9 | Остались TIME_DILATION и raw f64/testing пути; часы презентации ещё не единый источник | A3 |
| M-TIME-10 | Controller velocity игнорирует кривую; математический retarget не подключён к implicit | A4 |
| M-TIME-11 | Частота анимации/троттлинг вынесены владельцем | Вне текущего scope |
| M-TIME-12 | Спека motion-clock уточняет старую строку: rate ≥ 0, reverse отдельно; синхронизировать формулировки | A3 |
| M-INT-1, M-INT-2, M-INT-3, M-INT-5 | Библиотечные сегменты есть; C0/C1 и reversal должны пройти через реальные implicit/controller consumers | A4 |
| M-INT-4 | Продолжительность и атомарность отказа входят в красные robustness-контракты | A1 |
| M-INT-6 | Dismissible использует max constraints, а не фактический размер; Drawer/back gesture проверить вместе | A6 |
| M-INT-7, M-INT-8, M-INT-9, M-INT-10 | Базовые reverse/seek/cancel/completion тесты проходят; reentrant delivery и lifecycle ещё красные | A1, A4 |
| M-INT-11 | Finish — явный контракт, не эквивалент stop/reset; подтвердить выбранный interface и consumer | A1 |
| M-INT-12 | Конечность/bounds должны сохраняться при hostile curve/simulation и reentry | A1 |
| M-INT-13 | Аддитивная композиция вынесена при условии непрерывного retarget; условие ещё не закрыто в виджетах | A4, условный вынос |

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
| M-CRV-7 | Piecewise linear не найден как завершённый production-сценарий; нужна явная disposition строки | A7 |
| M-CRV-8 | Экстраполяция кривой вынесена; не смешивать с экстраполяцией интерполируемого значения | Вне текущего scope |
| M-CRV-9 | Cubic keyframes заменяют CatmullRom; подтвердить потребителя, а не только math tests | A7 |
| M-CRV-10 | Sampled spring curve не подтверждена; не помечать закрытой по наличию SpringSimulation | A7 |
| M-CRV-11, M-CRV-12 | Политика NaN/вне диапазона и checked construction/serde покрыты библиотечными тестами; сохранить при миграции | A7, A8 |

## Интерполяция и композиция

| Требования | Текущее основание / что ещё доказать | Пакет |
|---|---|---|
| M-INTP-1, M-INTP-2 | Интерполяция и Oklab реализованы; cross-crate suites этого прохода не запускались | A7, A9 |
| M-INTP-3 | Color хранит u8: premultiply реализован, обещание отсутствия 8-bit stepping отдельно не закрыто | A7, решение scope |
| M-INTP-4 | OkLCh и hue-политики вынесены | Вне текущего scope |
| M-INTP-5, M-INTP-6 | Matrix decomposition/Angle реализованы; проверить render-путь, а не только численные значения | A6, A9 |
| M-INTP-7, M-INTP-8 | TwoWayConverter/derive есть; миграция реальных implicit consumers ещё нужна | A4, A7 |
| M-INTP-9 | Порог покоя на тип не заменяется общим Tolerance или DPR scroll tolerance | A7, решение контракта |
| M-INTP-10 | Opt-in discrete values вынесены | Вне текущего scope |
| M-CMP-1, M-CMP-2, M-CMP-4, M-CMP-5 | Keyframes/hold/группы на общем времени/stagger есть; indicators дают consumers, нужны lifecycle/render проверки | A7 |
| M-CMP-3 | Repeat покрыт обычными тестами; completion/motion-policy проверить после core миграции | A1, A5 |
| M-CMP-6 | Полная WAAPI timing model вынесена | Вне текущего scope |
| M-CMP-7 | Решение удалить Compound ещё требует сверки финального surface: тип остаётся в baseline | A7 |
| M-CMP-8, M-CMP-10 | Switch/proxy delivery и reentry остаются среди красных контрактов | A1, A2 |
| M-CMP-9 | Reverse duration/curve есть; проверить сквозную активность и inherited velocity | A4 |
| M-CMP-11 | Phase animator условно вынесен, если keyframes закрывают выбранный сценарий; требуется демонстрация | A7, условный вынос |

## Доступность, владение, интеграция, качество

| Требования | Текущее основание / что ещё доказать | Пакет |
|---|---|---|
| M-A11Y-1, M-A11Y-2, M-A11Y-3, M-A11Y-4, M-A11Y-5 | Сквозная motion policy отсутствует на baseline; PR1515 — зависимость host preferences, не доказательство animation поведения | A5 |
| M-A11Y-6 | Отдельная cross-fade preference вынесена | Вне текущего scope |
| M-OWN-1 | Run ownership/reentry частично покрыты обычными тестами; сохранить при смене owner model | A1, A2 |
| M-OWN-2, M-OWN-6 | Owning lifetime и teardown не закрыты; есть красные контрактные тесты | A2 |
| M-OWN-3, M-OWN-5, M-OWN-8, M-OWN-9 | Dispose/activity/panic/delivery order воспроизводимо нарушают intended contracts | A1 |
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
| M-QLT-4 | Бенчи есть; измерить конечный активный путь на одинаковой конфигурации | A8 |
| M-QLT-5 | Allocation test проходит только для зафиксированных workloads; расширить доказательство по принятому scope | A8 |
| M-QLT-6 | Документацию синхронизировать с конечным поведением и scope | A8 |
| M-QLT-7 | 137 doctests проходят, 0 ignored на baseline | A8, A9 |
| M-QLT-8 | animation_it существует; окончательную раскладку public/private tests проверить при миграции | A8 |
| M-QLT-9 | MotionSpec/AnimatedValue и часть surface ещё требуют production callers либо явного удаления | A4, A7 |
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
