# render-proof — требования (уровень 1)

- **Статус:** черновик (ревью: approve with fixes, правки внесены)
- **Дата:** 2026-10-05 · **База:** `main` @ `4915054c8`
- **Уровень 0:** [../release/requirements.md](../release/requirements.md) R9; также R16 (#1043).
- **Issues:** #1043 (critical) в scope, плюс остатки #1148 для Win32. #1037 — только нативная
  половина. #1039, #1041 вне scope. #1149 закрыт тестами, #1147 отложен (см. ниже).

Спека задаёт, как доказать, что экраны Notes рисуются через wgpu правильно: readback
совпадает с эталоном по фиксированному правилу, а точки выборки и негативные контроли
отличают правильную отрисовку от сломанной. Сюда же входит закрытие #1043 (время жизни
native window в конструкторе `Renderer`) с исполненным доказательством.

## Решения

- **ADR-0087 §2 заменён.** §2 делал CPU-бэкенд путём для golden-изображений
  (`docs/adr/ADR-0087-raster-contract-and-cpu-backend.md`). План `docs/plans/engine-foundation-implementation.ru.md`
  (2026-10-01, строки 4–5) оставляет wgpu единственным renderer, а CPU oracle исключает из
  плана. Golden-доказательство идёт через wgpu. Новое ADR с `Supersedes: ADR-0087` для §2 —
  обязательство design.
- **Допуск на адаптер (бывший Q1).** Решается замером до приёмки. Первая задача фичи — замер
  lavapipe, WARP и железа на Notes. До замера R4 задаёт только структуру правила, без чисел.
- **CI (бывший Q2).** В этом релизе: локальный `cargo xtask gpu-test` и нативный прогон на
  Windows. Перенос набора в job `live-smoke` меняет `.github/workflows/` — это follow-up,
  нужно одобрение владельца.
- **RTL (бывший Q5).** Явная обёртка `Directionality` в тестовой фикстуре. Сам showcase не
  RTL: он не выставляет направление и не доказывает RTL сам по себе.

## Пользовательский сценарий

1. **Автор приложения.** Разработчик, у которого в dev-dependencies есть `flui` с фичей
   `testing`, монтирует своё дерево через `flui::testing::widgets::lay_out` (с
   `MountOptions`), прокручивает кадры и получает RGBA-readback одним вызовом. Затем сравнивает
   его с PNG из своего репозитория по явному правилу и точкам выборки. Если GPU нет, тест
   получает типизированную ошибку, а не молчаливый `Ok`. Эталон меняется только явной
   локальной командой.
2. **Доказательство релиза.** Мейнтейнер на SHA релиза запускает `cargo xtask gpu-test`
   (`FLUI_REQUIRE_GPU=1`) и нативный `windows-notes`. Отчёт R16 прикладывается к
   датированному прогону R14 уровня 0.

## Текущее состояние

- **Как readback получает кадр.** Только offscreen, без окна:
  `flui_engine::HeadlessRenderer::new()` (`crates/flui-engine/src/headless.rs:72`) создаёт
  `Instance` без display handle и запрашивает адаптер `HighPerformance`
  (`headless.rs:104-111`). `render_layer_tree(&LayerTree, (w, h))` (`headless.rs:174`) рисует в
  `Rgba8Unorm` размером в device pixels на белом фоне (`headless.rs:161-170`). Facade-тест
  строит `LayerTree` на `PipelineOwner` из render objects, без widget-дерева
  (`tests/composited_layer_update_readback.rs:37-64`). Затем он создаёт renderer
  (`HeadlessRenderer::new`, `:122-123`), растеризует (`:103-104`) и сравнивает update-путь с
  repaint побайтно (`:134-145`). Эталонного изображения нет.
- **Устройство headless отличается от оконного.** Берётся устройство wgpu по умолчанию, а не
  `adapter::request_flui_device` (`headless.rs:121-132`). Headless не видит формат и
  sRGB-кодирование swapchain, DPR от `scale_factor` ОС и damage scissor оконного `Renderer`
  (`crates/flui-engine/src/renderer.rs:1938-1946`). Это закрывает R15.
- **Gate.** Фича `gpu-readback-tests` (`Cargo.toml:769-776`, `:916-919`); запуск только через
  `cargo xtask gpu-test` (`tools/xtask/src/tasks.rs:530-553`) с `FLUI_REQUIRE_GPU=1`. Правило
  «без адаптера — падение» живёт в `crates/flui-engine/src/test_support.rs:270-296` и сейчас
  `pub(crate)`. CI без GPU и только на Linux (`docs/testing.md:1050-1056`). Устаревшие
  комментарии: `tasks.rs:1048` (CI на WARP), `Cargo.toml:771-772` (`gpu-test` на WARP в CI),
  `Cargo.toml:775` (CI запускает `gpu-test`).
- **Программный адаптер.** Engine не включает `force_fallback_adapter`: растеризатор выбирает
  хост (`crates/flui-engine/src/adapter.rs:27-38`). В CI lavapipe есть только в `live-smoke`
  (`.github/workflows/ci.yml:637-643`).
- **Notes offscreen.** По частям да, через публичный API — нет. `flui::testing` экспортирует
  `widgets`, `BuildCapabilities`, `HeadlessBinding`, `MountOptions`, `MountOwners`, `Mounted`,
  `a11y`, `replay` и `rendering` (`src/testing.rs:8-25`); readback там нет. `flui-engine` у
  facade только в dev-dependencies (`Cargo.toml:719`), а `flui-testing` от engine не зависит.
  Цепочка `HeadlessBinding` → `LayerTree` → `HeadlessRenderer` собрана в
  `examples/screenshot.rs:77-125`, но без Notes. `LaidOut::layer_tree()` есть
  (`crates/flui-testing/src/widgets.rs:1262`). Notes монтируется на 640×720
  (`tests/fixtures/notes_flow.rs:27-35`); экраны — `Route` (`examples/two_screens/tree.rs:25`),
  Home (`:257`), Note (`:344`), Settings (`:188`).
- **DPI.** Production берёт физический размер от ОС и выводит логический как физический/DPR
  (`crates/flui-rendering/src/view/configuration.rs:62-66`). DPR попадает в корневой
  `TransformLayer` (`configuration.rs:136-138`, `render_view.rs:202-206`). В harness DPR
  жёстко равен 1.0 (`crates/flui-testing/src/realm.rs:146-148`), а размер поверхности
  округляется `ceil` от логического (`widgets.rs:275-290`).
- **Пиксели.** Content quads рисуются на дробных позициях с AA, snapping по ADR-0098 §6 не
  сделан; паника в paint слоя отравляет кадр (`crates/flui-engine/ARCHITECTURE.md:983-993`).
  Между растеризаторами AA-края расходились до 48 на канал, текст — из-за шрифтов
  (`docs/testing.md:827-851`); теперь текст только на встроенных шрифтах (`:856-866`).
- **Damage.** `partial_equals_full_inside_damage`
  (`crates/flui-engine/src/damage_readback_tests.rs:928`) уже гонит production `LayerDiffer`
  по двум сценам. Сцены собраны вручную, внутри damage допуск ±2 (`:1023-1034`).
- **#1043.** Engine-часть сделана по ADR-0063: `Renderer::new(impl WindowTarget)`
  (`renderer.rs:569`, `crates/flui-engine/src/window_target.rs:43`), `SurfaceLease`,
  compile-fail фикстура `crates/flui-engine/tests/compile_fail/renderer_new_rejects_borrowed_window.rs`,
  Win32 отказывает в handle разрушенного HWND
  (`crates/flui-platform/src/platforms/windows/window.rs:1384-1386`). Порядок освобождения
  исполняется только на X11/Wayland (`tools/live-smoke/src/self_close.rs:107-140`). #1148
  закрыт, но его пункты Win32 2–3 (device loss после close, `close()` из callback) прогоном
  не подтверждены. Сам `windows-notes` закрывает Notes по Alt+F4
  (`tools/xtask/src/device/windows_notes.rs:221-228`).

### Решение по связанным issues

- **#1037 — частично.** Производитель damage подключён на native (`LayerDiffer` в raster lane,
  `crates/flui-app/src/app/raster_lane.rs:202-207`, `:241-243`). Остались две вещи. Web
  `DirectSink` всегда делает полную перерисовку (`raster_lane.rs:486-491`) — вне scope. В
  реальном окне частичный кадр не наблюдался (ADR-0087, раздел Verification) — это нативная
  половина, её закрывает R15.
- **#1039, #1041 — нет.** Производительность view и layout, на пиксели не влияют.
- **#1149 — закрыт тестами.** Miri над `surface_lease` и `cancelling_renderer_new` уже в
  `miri_plan` (`tasks.rs:555-572`); повторный probe в `recover()` покрывает R13.
- **#1147 — отложен** в спеку `teardown` (R10 уровня 0): утечка обёрток окон, не поверхности.

## Требования

Тиры по `docs/testing.md`: **GPU readback** (локально, `cargo xtask gpu-test`), **Widget**
(`HeadlessRealm`, CI), **Unit** (in-src, CI), **native** (`cargo xtask device windows-notes`,
датированный ручной прогон на Windows).

- **R1.** КОГДА тест вызывает readback смонтированного дерева через `flui::testing` (за
  фичей, которая подтягивает engine), СИСТЕМА ДОЛЖНА возвращать RGBA8 последнего
  закоммиченного кадра размером в device pixels. Тест: `notes_home_readback_has_the_committed_size`,
  тир GPU readback. Обязательства design: тот же `layer_walk`, что у окна, и ADR,
  заменяющее ADR-0087 §2.
- **R2.** КОГДА каждое из состояний Notes (Home: загрузка, ошибка с Retry, список, компактные
  строки; Note с ошибкой валидации; Settings) отрисовано при DPR 1.0, СИСТЕМА ДОЛЖНА давать
  readback, который проходит R3 и R4 против эталона. Одна таблица, строка на состояние. Тест:
  `notes_screens_match_their_references`, тир GPU readback.
- **R3.** КОГДА выполняется сравнение, СИСТЕМА ДОЛЖНА сначала проверить точки выборки. Каждая
  точка стоит не ближе 2 device px к краю и попадает в элемент, который различает экраны:
  заливка app bar, фон выбранной строки, глиф заголовка, рамка поля с ошибкой. Ожидаемый цвет
  берётся из констант темы, геометрия — из layout probe, не из эталонного PNG. Допуск ±2 на
  канал. Тест: строки R2, тир GPU readback.
- **R4.** КОГДА сравнивается весь кадр, СИСТЕМА ДОЛЖНА применять одно правило, записанное в
  тесте константами. Структуру выбирает замер, из трёх вариантов: (а) одна константа на все
  адаптеры; (б) она же плюс маска, исключающая glyph runs, при этом текст покрыт глиф-точками
  R3; (в) эталон на класс адаптера. Общей полосы исключения вдоль краёв нет: края проверяют
  R6 и R8. Числа фиксируются после замера, и каждый контроль R5 обязан их превышать. Отказ
  называет число и координаты первых N расхождений и путь к дампу (`FLUI_READBACK_DUMP_DIR`).
  Тест: строки R2, тир GPU readback.
- **R5.** КОГДА в отрисовку внесена известная поломка, СИСТЕМА ДОЛЖНА провалить именно ту
  проверку, которую называет строка контроля. Если контроль прошёл или провалился по другой
  причине (например, из-за несовпадения размера), тест падает. Поломка вносится мутацией
  закоммиченного `LayerTree` или test-only переключателем painter, никогда правкой production
  кода. Тест: `notes_reference_rejects_broken_renders`, тир GPU readback.

  | Контроль | Должен провалить |
  |---|---|
  | удалён один слой | R4 |
  | поддерево сдвинуто на 1 логический px | R3 |
  | поддерево сдвинуто на ½ device px | R4 и краевая точка R8 |
  | clip расширен на 1 device px | краевая точка R8 |
  | premultiplied ↔ straight alpha | альфа-точка R6 и R3 |
  | смешивание в linear вместо sRGB | точка градиента R6 |
  | неверная альфа opacity | R3 |
  | пропущен clip | R4 |
  | DPR удвоен в корневой матрице при 1.0 и при 1.5 (размер readback сохранён) | R3 |
  | зеркальная раскладка (эталон LTR против RTL) | R3 (R9) |

- **R6.** КОГДА рендерится синтетическая калибровочная сцена (50 % альфа поверх известного
  цвета, градиент из двух стопов, скруглённый clip), СИСТЕМА ДОЛЖНА давать аналитически
  вычисленные значения в точках ±2, а краевые точки на точных дробных позициях — ожидаемое
  смешанное значение. Тест: строка `calibration` в `notes_reference_rejects_broken_renders`,
  тир GPU readback.
- **R7.** КОГДА кадр Notes после локального изменения (компактные строки, ввод в поле)
  отрисован с damage, который production `LayerDiffer` вычислил из двух закоммиченных
  `LayerTree` harness, СИСТЕМА ДОЛЖНА давать readback, равный полной перерисовке. Вне damage
  побайтно, внутри — по правилу существующего теста (±2 только с записанной в тесте причиной,
  иначе 0). Тест: расширение `partial_equals_full_inside_damage` строкой Notes, тир GPU
  readback.
- **R8.** КОГДА harness монтирует дерево с DPR 1.5 или 2.0, СИСТЕМА ДОЛЖНА выводить
  логический размер так же, как production: физический размер задан целым, логический равен
  физическому/DPR (`ViewConfiguration::from_size`). Readback имеет физический размер. Строки
  R2 повторяются для DPR 1.5 и 2.0 со своими эталонами. На границе с дробной device-позицией
  (DPR 1.5, нечётная логическая ширина) отдельная краевая точка проверяет смешанное значение.
  Параметр DPR добавляется в `MountOptions`. Тесты: столбец DPR в
  `notes_screens_match_their_references` (GPU readback) и `mount_at_dpr_derives_logical_size`
  (Widget).
- **R9.** КОГДА фикстура оборачивает Notes в `Directionality` с RTL, СИСТЕМА ДОЛЖНА давать
  зеркальную раскладку: точки R3 видят начало заголовка и ведущую иконку у правого края.
  Тест: строки `*_rtl` в `notes_screens_match_their_references`, тир GPU readback.
- **R10.** КОГДА адаптера или устройства нет, СИСТЕМА ДОЛЖНА возвращать из API R1
  типизированную ошибку и решать skip/fail той же функцией, что и наборы `gpu-test`; API R1
  её экспортирует. При `FLUI_REQUIRE_GPU` тест падает. Без переменной он пропускается со
  строкой `skipping: no usable GPU` и в доказательство релиза не засчитывается. Тест:
  `notes_readback_without_adapter_is_an_error`, тир Widget, без GPU.
- **R11.** КОГДА устройство потеряно во время захвата или paint слоя паникует, СИСТЕМА ДОЛЖНА
  возвращать ошибку вместо пикселей и не отдавать частичный кадр как успешный. Следующий
  захват на новом renderer проходит R3. Матрица: каждый отказ отдельно, оба подряд,
  следующая операция после сдерживания. Тест: `capture_failure_matrix`, тир GPU readback
  (in-src, нужен приватный шов).
- **R12.** КОГДА поверхность устарела или окно изменило размер между acquire и present,
  СИСТЕМА ДОЛЖНА пропустить кадр (`SurfaceStale`) и не публиковать его, а следующий кадр
  отрисовать в новом размере. Тесты: строки fake `RasterBackend` в `surface_lifecycle_matrix`
  (`crates/flui-app/src/app/runner/surface_lifecycle.rs:790`, тир Unit) и шаг «нет
  растяжения после resize» в `windows-notes` (native, точки R3 после шага resize).
- **R13.** КОГДА окно на Windows закрыто, СИСТЕМА ДОЛЖНА не трогать разрушенный HWND. Три
  пути: (а) поверхность освобождается раньше выхода цикла при закрытии через Alt+F4 и через
  `close()`; (б) device loss, вызванный после закрытия, даёт из `recover()`
  `SurfaceTargetUnavailable`; (в) `close()` из callback, пока поверхность арендована,
  завершается безопасно (`Lost`/`Outdated`), без падения. Тест: расширение существующего шага
  Alt+F4 в `windows_notes.rs` проверками порядка `surface_released`, плюс (б) и (в), тир
  native. Вместе с compile-fail фикстурой это закрывает #1043 со ссылкой на лог.
- **R14.** КОГДА тест запущен с явной переменной обновления, СИСТЕМА ДОЛЖНА переписать эталон
  и тест всё равно провалить. Без переменной эталон не пишется никогда; CI её не выставляет.
  Тест: `reference_update_requires_explicit_opt_in`, тир Widget (без GPU, на записи файла).
- **R15.** КОГДА `windows-notes` показывает Notes в реальном окне при системном масштабе 100 %
  и 150 %, СИСТЕМА ДОЛЖНА давать снимок окна (захват экрана из xtask, например `xcap`),
  который проходит только точки R3 (без сравнения с эталоном). Trace прогона содержит
  `Damage scissor applied` хотя бы для одного кадра после локального изменения. Тест: шаг
  `windows-notes`, тир native.
- **R16.** КОГДА завершается прогон доказательства релиза, СИСТЕМА ДОЛЖНА выдать отчёт:
  адаптер (имя, backend, драйвер), число сравнённых пикселей на строку, итог каждого
  негативного контроля R5 с проваленной проверкой, а также масштабы и итог R15. Тест: отчёт
  формирует `gpu-test`; проверка — что в отчёте есть все поля, тир Widget (на фиктивных
  результатах).

**Правило для PR (review, а не тест).** Каждый изменённый эталон указан в описании PR с
причиной и diff-изображением. Сделать это gate можно только командой `cargo xtask` плюс шаг CI
(AGENTS.md «A new gate»), что меняет workflow, — это follow-up с одобрения владельца.

## Сценарии отказа

| Сценарий | Правило | Где |
|---|---|---|
| Нет адаптера или устройства | Падение при `FLUI_REQUIRE_GPU`, иначе явный skip вне доказательства | R10 |
| Device lost (захват; после закрытия окна) | Ошибка, не пиксели; `SurfaceTargetUnavailable` | R11, R13 |
| Surface outdated, resize посреди кадра | Кадр пропущен, следующий в новом размере без растяжения | R12 |
| DPR 1.0 / 1.5 / 2.0 | Округление как в production, эталон на DPR; масштаб ОС 100/150 % | R8, R15 |
| Дробные границы | Краевые точки на точных позициях и калибровка | R6, R8 |
| RTL-текст | Обёртка `Directionality` в фикстуре; эталон LTR обязан провалиться | R9, R5 |
| Допуск скрывает регрессию | Каждый контроль проваливает названную проверку | R5 |
| Отличие адаптеров | Структура правила по замеру; адаптер в отчёте | R4, R16 |
| Обновление эталона | Только явная переменная; причина в PR | R14 |
| `close()` из callback | Безопасный отказ поверхности | R13 |

## Вне scope

- Web `DirectSink` с полной перерисовкой (остаток #1037); #1039, #1041; #1147 (в `teardown`).
- Изоляция паники одного слоя вместо отравления кадра: смена контракта, отдельное ADR.
- Snapping content quads (ADR-0098 §6); при его приходе эталоны пересъёмываются по R14.
- GPU-job в CI и перенос набора в `live-smoke` (follow-up с одобрения владельца);
  сертификация macOS, Linux, Web, Android и iOS (решение D1 уровня 0).
- Pixel-golden для demo Material и Cupertino: остаются структурные снимки.
- CPU-бэкенд и CPU oracle (ADR-0087 §2 заменён).

## Открытые вопросы

1. Куда идёт API R1: в `flui::testing` за новой фичей (`testing-gpu`), которая подтягивает
   `flui-engine` в dev-граф пользователя, или в `flui-testing` с зависимостью на engine
   (проверить `cargo xtask reach`)?
2. Брать ли headless-устройство через `request_flui_device`, чтобы readback проверял те же
   возможности, что и окно, или различие остаётся и документируется (R15 закрывает его лишь
   частично)?
3. Какой адаптер считать эталонным для строк R2, если замер выберет вариант (в) — железо
   мейнтейнера или WARP, который воспроизводится на любом Windows?
