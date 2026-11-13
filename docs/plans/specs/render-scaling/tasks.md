# render-scaling: задачи

- **Статус:** утверждена 2026-11-11; исполнение. Сделано: RS1, RS3, RS4, RS5, RS6, RS7, RS8, RS9, RS14, RS15a, RS15c1, RS15c2, RS19a, RS19b. В работе: RS8c
  (update RS8 выше 1,6× принят автором спеки). Следующие: RS16, после него RS15b; счётчик обхода движка — в RS12.
- **Design:** [design.md](design.md); требования и ворота G1–G7: [requirements.md](requirements.md)
- **База:** `main` @ `da69efa`. Пути и строки в ADR даны на `e5ba8fe8f`; перед правкой сверить.

## Правила исполнения

- Задача = ветка = PR, проверки по `AGENTS.md`, «Checking a change» и «Before each push».
- PR не добавляет ADR. Если шаг не строится без решения, которого в ADR-0304, ADR-0305 и ADR-0306
  нет, исполнитель останавливается и присылает вопрос координатору.
- Тест задачи падает при откате её производственной правки. Имена тестов взяты из разделов
  Confirmation трёх ADR; менять имя можно только вместе с ADR.
- Счётчик, названный в задаче, добавляется в `FrameReport::counters()` и в
  `crates/flui-widgets/perf/baseline/` тем же PR; изменение чужих строк baseline объясняется в
  теле PR построчно.
- Каждый PR правит пункт своего ADR в `docs/gaps.md`: убирает построенное и называет остаток.
  PR, после которого пункт пуст, удаляет его и ставит записи `Implemented (дата)`.
- Снимок `*.snap` перегенерируется только там, где задача это разрешает явно.
- GPU-тесты идут под `cargo xtask gpu-test` (lavapipe, llvmpipe); тело PR называет платформы, на
  которых изменение ещё не запускалось.
- RS5, RS8 и RS15 перед слиянием ревьюит тред «Исправить курс и обогнать аналоги».

## Дорожка D: display list (ADR-0306)

### RS1. Неконечный ввод, `draw_picture`, порядок очистки

- **Что:** ADR-0306 §4 (отказ неконечному вводу при записи, по виду операции, со счётчиком
  `non_finite()`), §6 (удалить `Canvas::draw_picture`, composer переходит на `append_isolated`),
  §7 (`clear` сначала сбрасывает кэшированные границы, потом роняет команды).
- **Где:** `crates/flui-painting/src/canvas/drawing.rs`, `display_list/mod.rs`,
  `crates/flui-rendering/src/pipeline/owner/paint.rs` (splice), `crates/flui-engine`.
- **Тесты:** `a_non_finite_op_is_refused_by_kind`,
  `a_destructor_that_panics_during_reset_leaves_the_canvas_empty` (`recording_contract`);
  `a_non_finite_op_is_omitted_and_the_frame_draws`; `inline_picture_replay_owns_its_root_clips`
  переписан над `append_isolated`.
- **Ворота:** снимки paint без перегенерации; `rg draw_picture crates packages docs` пуст.
- **Зависит:** нет. **Размер:** M.

### RS2. Интернированные трансформы и размер операции

- **Что:** ADR-0306 §1–§3, §5: команда это приватный `TransformId` и `DrawOp`; геометрия локальная
  `f64`; `DRRect`, `ImageRegion`, `ImageFiltered` под `Box`; `Path` один указатель; три
  `const`-проверки размера; читатель отдаёт трансформ и операцию.
- **Где:** `crates/flui-painting/src/display_list/command.rs`, `paint/path.rs`,
  `crates/flui-engine/src/layer_dispatcher.rs` и читатели display list.
- **Тесты:** `ops_under_one_transform_share_its_entry`, `appended_commands_keep_their_transforms`.
- **Ворота:** G7; бенч `flui-rendering/benches/paint.rs` по инструкциям не хуже базы; снимки paint
  без перегенерации.
- **Зависит:** RS1. **Размер:** L.

## Дорожка S: сцена (ADR-0304)

### RS3. `Region` и исчерпывающий `DamageRegion`

- **Что:** ADR-0304 §5: `DamageRegion { Unchanged, Region, Full(FullReason) }`, `Region` до 8
  непересекающихся прямоугольников, `DamageRect::try_covering`, `FullReason`, `ResyncReason`.
  Производитель пока прежний (`LayerDiffer` даёт один прямоугольник как `Region` из одного), движок
  планирует по рамке региона. Удалить wildcard-ветку в `raster_owner.rs`.
- **Где:** `crates/flui-layer/src/scene_snapshot.rs`, `crates/flui-engine/src/damage.rs`,
  `raster_owner.rs`, `crates/flui-app/src/app/raster_lane.rs`.
- **Тесты:** таблица `region_contract` (`a_ninth_rect_merges_the_cheapest_pair`,
  `merged_rects_stay_pairwise_disjoint`, `a_non_finite_rect_escalates_to_full`,
  `full_keeps_its_first_reason`); `union_table` и `bounds_round_outward_with_aa_margin` переписаны.
- **Ворота:** ни одного `_ =>` над `DamageRegion`, `FullReason`, `ResyncReason`;
  `damage_readback_tests` проходят без правки ожиданий.
- **Зависит:** нет. **Размер:** M.

### RS4. Кусок принадлежит одному render-объекту

- **Что:** ADR-0304 §2, первая половина: composer запечатывает открытый прогон при смене рисующего
  render-объекта. Запись `RenderId` того, кто толкнул эффект, уходит в RS8: её единственный
  читатель, корреляция последователя в `BoundaryRecord`, появляется там.
- **Где:** `crates/flui-rendering/src/pipeline/owner/paint.rs` (слияние прогонов).
- **Тесты:** `a_picture_run_holds_one_render_objects_output` (новый, `flui-rendering/tests`).
- **Когда:** в одной серии с RS8, сразу перед его производителем патчей и в тот же день. На
  сегодняшней пересадке `LayerTree` захват клонирует по картинке на лист:
  `paint/rotated_box_turn_change/inline` при N = 1 000 даёт update 3.13 µs → 50.0 µs и repaint
  478 µs → 676 µs, а `slide_transition_tick.layers_reused` 1 → 30. Читателя у кусков по владельцу
  до RS8 нет: RS5–RS7 строят модель сцены в `flui-layer` из тестовых патчей.
- **Ворота:** вместе с RS8 update и repaint `paint/rotated_box_turn_change/inline` при N = 1 000 не
  хуже main (3.13 µs, 478 µs, в пределах шума), `slide_transition_tick.layers_reused` не больше 30;
  readback без изменений; снимки paint перегенерируются только в границах прогонов. Если RS8 не
  возвращает update, слияние останавливается и вопрос о зерне ADR-0304 §2 идёт автору спеки.
- **Зависит:** нет; сливается с RS8. **Размер:** M.

### RS5. Персистентная таблица, `SceneVersion`, `apply` с проверкой

- **Что:** ADR-0304 §1–§3: таблица с копированием пути (форма в [design.md](design.md)),
  `SceneVersion`, `SceneNodeId`, `BoundaryContent`, `Entry`, `Chunk`, `ContentVersion`, `Lineage`,
  `Placement::try_new`, `SceneOp`, `SceneWriter`, `SceneOwner::apply` с проверкой «всё или ничего»
  и `SceneError`; три индекса (ссылки, пользователи текстур, узлы с фильтрами);
  `SceneVersion::flatten()` за фичей `testing`. Damage в этой задаче всегда `Full(FirstFrame)`, после
  `renew` `Full(Resync(_))`. Корень сцены владелец создаёт сам, `Retire` корня отказывает.
  Границы (`BoundaryContent::bounds`, `subtree_bounds`), `filters` и `Chunk::reads_destination`
  нужны только следу и `read_closure` и приходят с RS6.
- **Где:** новые модули `crates/flui-layer/src/scene/`; `Lineage` минтит `AtomicIdMint` со строкой
  `counter` в `globals` манифеста `flui-layer`.
- **Тесты:** модельная проверка таблицы против `BTreeMap` и разделение поддеревьев по
  `Arc::ptr_eq`; из `scene_apply_contract`: `a_slot_naming_its_own_ancestor_is_refused`,
  `a_stale_generation_is_refused`, `a_node_named_by_two_slots_is_refused`,
  `a_refused_patch_leaves_the_version_and_log_unchanged`, `a_node_of_another_owner_is_refused`; из
  `content_version_contract`:
  `a_replaced_owner_never_reports_unchanged`,
  `an_exhausted_mint_renews_the_lineage_and_publishes_full`.
- **Ворота:** стоимость `apply` зависит от патча, а не от размера сцены (`scene_table_cost`, счётчик
  копий в тесте): (1) патч из k операций копирует не больше k × высота узлов таблицы, где высота
  равна ceil(log32(ёмкость)), на 100, 1 100, 30 000 и 100 000 узлах; (2) на одной высоте те же
  операции копируют одно и то же число узлов при любом числе узлов (1 100 против 30 000,
  равенство); (3) k операций в одном листе копируют один путь, то есть высоту узлов; (4) высота не
  фиксируется, рост на границе ёмкости (1 024 → 1 025 слотов) добавляет по одному новому узлу на
  уровень и ничего не копирует; (5) `Publish` родителя с теми же слотами и одним новым копирует
  одно и то же число узлов при 40 и 1 000 детях; (6) перерисовка с теми же ссылками и текстурами не
  пишет индексы; (7) новая граница над поддеревом из n узлов обновляет n + 1 глубину
  (`depth_updates`). `send-impls` без новых строк.
- **Зависит:** нет. **Размер:** L. **Ревью сильной модели.**

### RS6. Damage как след патча, `read_closure`, последователи

- **Что:** ADR-0304 §4 и §6: след `Publish`, `Place`, `Retire`; `BoundaryContent::bounds`,
  `subtree_bounds` с ростом вверх, `filters` и `Chunk::reads_destination`;
  разрешение последователей после операций; одна функция `read_closure` с замыканием до
  неподвижной точки; правила `differ.rs:480-507` и `frame_protocol.rs:215-300` удаляются, оба
  места зовут `read_closure`.
- **Где:** `crates/flui-layer/src/scene/`, `damage/differ.rs`,
  `crates/flui-engine/src/frame_protocol.rs`.
- **Тесты:** `a_place_damages_the_subtree_under_both_placements`,
  `an_effect_chain_change_damages_every_slot_under_it`,
  `a_follower_resolves_after_its_leaders_boundary_moves_parents`;
  `the_renderer_kernel_stays_inside_read_closure`; property-тест
  `patch_damage_covers_diff_damage` с подсаженным патчем, теряющим след одной операции.
- **Ворота:** property-тест на 10 000 случаев зелёный и красный на подсаженном дефекте; счётчики
  `followers_resolved`, `scene_depth_updates`.
- **Зависит:** RS3, RS5. **Размер:** L.

### RS7. Журнал ожидания, публикация и подтверждение

- **Что:** ADR-0304 §7 на стороне владельца: `PendingLog` на 16 записей со сворачиванием двух
  старейших, `publication()`, `completed()` (только вперёд, чужая линия игнорируется, заменённое
  содержимое освобождается после фиксации состояния), `renew()`, `SurfaceKey`.
- **Где:** `crates/flui-layer/src/scene/owner.rs`.
- **Тесты:** таблица `pending_log_contract` (все пять строк ADR).
- **Ворота:** счётчик `pending_log_folds`; `Drop` заменённого содержимого, который зовёт владельца
  снова, не находит заимствований (тест с пробой повторного входа).
- **Зависит:** RS6. **Размер:** M.

### RS8. Paint пишет патч, runtime публикует версию

- **Что:** ADR-0304 §8 и производящая половина §7: `PipelineOwner::run_frame` берёт `SceneWriter`,
  красит только границы из очереди, чистая сдвинутая граница даёт `Place`, `BoundaryRecord` вместо
  захватов ADR-0253 §2 с `RenderId` того, кто толкнул каждый эффект (вторая половина RS4), граница с
  `Leader` или `Follower` удерживается; `FrameSink::submit`
  принимает `ScenePublication`; `SceneSnapshot` несёт `base` и `SceneVersion`; полоса растра
  принимает damage по базе и отдаёт движку `Scene` из `flatten()` (временный остаток, см. design).
  Удаляются сборка `LayerTree` в paint, `ContentToken`, `BoundaryStamp`, производственный вызов
  `LayerDiffer`. Отказ патча и паника в paint идут по ADR-0304 §10.
- **Где:** `crates/flui-rendering/src/pipeline/owner/paint.rs`, `crates/flui-runtime/src/sink.rs`,
  `ui_runtime/frame.rs`, `crates/flui-app/src/app/raster_lane.rs`,
  `crates/flui-layer/src/scene_snapshot.rs`, `tree/stamp.rs`, `crates/flui-testing`.
- **Тесты:** `a_clean_boundary_emits_no_operation`,
  `a_paint_panic_applies_no_patch_and_keeps_the_presented_version`,
  `a_boundary_holding_a_leader_is_retained`,
  `a_boundary_painted_alone_reads_the_anchor_its_parent_gave_it`,
  `a_refused_submit_keeps_its_damage_owed`, `a_refused_patch_rebuilds_under_a_new_lineage`,
  `a_hot_reload_renews_every_scene_lineage`, `a_base_mismatch_renders_full`;
  `retained_frames_match_a_full_repaint_over_random_trees_and_marks` сравнивает `flatten()`.
- **Ворота:** сценарий `caret_blink_in_10k_list`: `nodes_painted` и `scene_ops` равны на 1 000 и
  10 000 элементов; `damage_rects`, `damage_full`, `scene_nodes_visited` введены;
  `scene_depth_updates` показан на кадре, где начало opacity или slide вставляет границу над
  большим поддеревом (заметный скачок возвращает вопрос о хранении глубины автору спеки); счётчик
  аллокаций кадра показывает, что `apply` с N записями на одну текстуру или одну ссылку аллоцирует
  столько же, сколько с одной (корзина индекса расширяется на месте); readback-наборы
  без правки ожиданий.
- **Зависит:** RS4 (в той же серии), RS7. **Размер:** XL. **Ревью сильной модели.**
- **Серия:** три PR, каждый зелёный сам по себе. Первый: RS4 (сливается только после зелёного
  второго). Второй: производитель патчей с публикацией и удалением старых форм выше; общие ворота
  RS4 меряются на его голове. Третий: RS8c. Конвертера `LayerTree` → `SceneVersion` нет: он стоил
  бы O(сцена) дважды на кадр и уходил бы следующим PR. Если второй выходит за 3 000 строк диффа,
  от него отделяется проводка `BoundaryRecord` и `SceneWriter` за прежней сборкой дерева, без
  второго производственного пути: публикует по-прежнему старый.

### RS8c. Буферы paint и сборки сцены живут между кадрами

- **Что:** `Vec<FragmentOp>` нарисованного узла, команды display list, буферы патча и детей слоёв
  сохраняют ёмкость между кадрами (профиль: `paint.rs` `FragmentOp::Child`/`Push`/`Run`,
  `display_list/mod.rs` `commands.push`, `paint.rs` списки границ). `BoundaryRecord` живёт за
  индексом слэба, чтобы записи не перемещались; `node_links` и область границы кэшируются на
  границу в пределах одной версии.
- **Ворота:** строка baseline `xtask perf --check`, падающая на коде до пулов: 0 аллокаций paint
  и сборки сцены на `slide_transition_tick` и на кадре без изменений; update
  `paint/rotated_box_turn_change/inline` 2.13 / 3.08 / 12.7 µs при N = 1 / 1 000 / 10 000, а цель,
  которую не взять, получает в строке статуса причину и разбивку callgrind.
- **Зависит:** RS8. **Размер:** M.

### RS9. Движок обходит версию

- **Что:** ADR-0304 §9, первая половина: движок идёт по удержанной версии и отсекает узлы по
  следу поддерева; `flatten()` и `LayerDiffer` уходят за `testing`; самолечение перекрытия в
  `renderer.rs:2654-2675` становится debug-проверкой.
- **Где:** `crates/flui-engine/src/renderer.rs`, `layer_walk.rs`, `frame_protocol.rs`,
  `crates/flui-app/src/app/raster_lane.rs`.
- **Тесты:** `a_raster_panic_renders_the_next_frame_in_full`; существующие
  `superseded_damage_folds_into_the_survivor` и
  `a_moved_box_repaints_its_old_and_new_positions_only` остаются.
- **Ворота:** G3 без `chunks_recorded`; `scene_nodes_visited` в `caret_blink_in_10k_list` равен на
  двух размерах; `rg 'flatten\(' crates --glob '!**/tests/**'` находит только определение за
  `testing`.
- **Зависит:** RS8. **Размер:** L.

### RS10. Непоказанный кадр повторяется из удержанной версии

- **Что:** ADR-0304 §7, последний абзац: повтор после `NotShown` публикует удержанную версию;
  `mark_needs_full_repaint_for` уходит из повтора ADR-0068 §3, `wake_frame()` остаётся.
- **Тесты:** `a_frame_not_shown_is_retried_from_the_held_version`.
- **Ворота:** G6 (после RS12 вместе с `chunks_recorded = 0`).
- **Зависит:** RS9. **Размер:** S.

### RS11. Регион перерисовывается по прямоугольникам

- **Что:** ADR-0304 §9: `RegionPlan::{Bounds, Rects}`, свой scissor и своё отсечение на
  прямоугольник; каждый sink применяет damage публикации, включая `DirectSink`.
- **Где:** `crates/flui-engine/src/frame_protocol.rs`, `retained_target.rs`, `damage.rs`.
- **Тесты:** readback-строка `region_frames_match_a_full_frame` (два угла, полупрозрачное между).
- **Ворота:** G2; счётчик `region_passes`.
- **Зависит:** RS9. **Размер:** M.

### RS12. Кэш записей

- **Что:** ADR-0304 §9: `RecordingKey { content, raster, ambient, epoch }`, сдвиг на целые пиксели
  при воспроизведении, вставка после завершённой записи, вытеснение под бюджетом 64 МиБ,
  `ResourceEpoch`; при `FLUI_DAMAGE=off` кэш не читается.
- **Где:** `crates/flui-engine/src/command_ir.rs`, `renderer.rs`, новый модуль кэша.
- **Тесты:** `a_cached_chunk_replays_the_command_stream_it_recorded`,
  `a_chunk_moved_by_whole_pixels_hits_the_recording_cache`,
  `an_atlas_reset_misses_every_cached_recording`.
- **Ворота:** G1 целиком, G3, G4, G6; счётчики `chunks_recorded`, `recording_cache_hits`,
  `recording_cache_evictions`, `recording_cache_bytes`.
- **Зависит:** RS9, RS2 (состояние клипа применяется при воспроизведении). **Размер:** L.

### RS13. Текстуры, canvas-слой, оверлей и ввод читают версию

- **Что:** ADR-0304 §8 (конец) и §9 (конец): кадр текстуры повреждает только своих пользователей;
  display list canvas-слоя и оверлей производительности версионируются; input regions, наблюдение
  за лидерами и hit-test последователей читают применённую версию; ссылка инспектора называет узел
  его `SceneNodeId`, в котором уже есть lineage.
- **Тесты:** `a_texture_frame_damages_only_its_users`; строка сценария: оверлей без изменения чисел
  даёт `damage_rects = 0`.
- **Ворота:** пункт ADR-0304 в `docs/gaps.md` пуст и удалён.
- **Зависит:** RS12. **Размер:** M.

## Дорожка G: GPU-контекст (ADR-0305)

### RS14. `GpuContext` и исполнитель inline, headless на слотах

- **Что:** ADR-0305 §1, §10 (inline), §11: один `GpuContext` (`Send`, `!Sync`) у исполнителя растра;
  `HeadlessRenderer` рисует через offscreen-слоты. Оконные раннеры пока на `Renderer`.
- **Тесты:** `every_slot_shares_one_device_one_glyph_atlas_and_one_pipeline_per_format`;
  `static_assertions` у типа.
- **Ворота:** G5; `cargo xtask globals`, `send-impls`, `send-positions` без новых строк.
- **Зависит:** нет. **Размер:** L.

### RS15. Фабрика поверхностей, команды слота, расчёт по штампу

- **Что:** ADR-0305 §2–§6 без удаления оси ресурсов (она в RS19): `SurfaceFactory` (`!Send`),
  закрытые команды слота, расчёт кадра по штампу, темп и `SurfaceGeneration` на слот. PR идут в
  порядке (a), (c1), (c2), затем RS16, затем (b): (a) исполнитель, команды и кольцо расчётов в
  `flui-engine`; (c1) `DisplayConnection` каждого хоста и `OwnerPlatform::display_connection`;
  (c2) `SurfaceFactory`, `bootstrap` и оконные слоты, где отказ resize повторяется только после
  паники или переходной ошибки, а размер, который backend отклоняет всегда, пишет warn один раз;
  (b) `Queued`, `settle_frame` и выпуск `FrameEpoch` в `flui-runtime`. (b) идёт после RS16,
  потому что до потока растра `Queued` некому отвечать: RS16 сдаёт inline-вердикт тем же типом
  результата и через тот же один вход рантайма, которым (b) примет `Queued`, а эпоху, которую пока
  штампует дорожка `flui-app`, читает в одном месте, и (b) переносит это место в рантайм.
- **Тесты:** `a_surface_targets_display_equals_the_platforms_connection` (c1: X11 под `Xvfb`, Wayland
  под `weston`, headless); compile-fail `surface_factory_is_owner_local`,
  `surface_factory_rejects_borrowed_window`;
  `a_full_lifecycle_queue_returns_the_command`, `a_stale_slot_id_is_refused_after_detach`,
  `a_superseded_scene_is_dropped_after_the_inbox_lock_is_released`,
  `a_full_settlement_ring_holds_the_next_frame`,
  `a_stopped_executor_settles_every_pending_frame_failed`;
  `a_queued_frame_settles_by_its_stamp_and_a_stale_settlement_is_ignored` (`flui-runtime`);
  протокольные тесты `raster_owner` переписаны на слот (список в ADR-0305, Confirmation).
- **Ворота:** все перечисленные тесты; в `RasterExecutor`, слоте и `RasterHost` нет `Mutex`;
  единственный замок у inbox слота, держится только на обмен значений.
- **Зависит:** RS14; (b) ещё и RS16. **Размер:** L. **Ревью сильной модели.**

### RS16. Настольные раннеры на слотах

- **Что:** Win32, AppKit, Wayland и X11 раннеры `flui-app` работают через слоты; удаляются
  `with_backend`, `Arc<Mutex<RasterLane>>` и замыкание-хук; у окна нет своего `Renderer`.
- **Тесты:** `a_hidden_slot_is_not_acquired_and_settles_not_shown`.
- **Ворота:** `rg 'Arc<Mutex<RasterLane>>|with_backend' crates` пуст; два окна примера делят один
  device (G5 через оконный путь на Linux CI).
- **Зависит:** RS15a, RS15c, RS9. **Размер:** L. Windows и macOS: проверка типов и ночной раннер.

### RS17. Android и iOS на слотах

- **Что:** те же правки в мобильных раннерах.
- **Ворота:** кросс-проверка типов обеих целей; строки в `platform-parity.toml` не повышаются до
  прогона.
- **Зависит:** RS16. **Размер:** M.

### RS18. Web на inline-исполнителе

- **Что:** `DirectSink` удаляется; web рисует через тот же исполнитель и применяет damage
  публикации.
- **Ворота:** wasm-набор CI; `rg DirectSink crates docs` пуст.
- **Зависит:** RS16. **Размер:** M.

### RS19. Паника на команду, пересборка устройства, остановка, замер доли растра

Три части: (a) и (b) живут в `gpu_context.rs` и `raster_executor/`, ничего в `flui-app`, и
проверяются inline на потоке владельца так, чтобы тот же код был верен после переезда исполнителя
на поток растра; (c) ждёт RS16.

- **(a) Что:** ADR-0305 §7 и §9 в движке. Паника кадра сдерживается на команду и стоит общих
  кэшей (атлас глифов, кэши изображений и путей, пулы), но не пайплайнов: библиотека пайплайнов и
  пайплайны blit переживают раскрутку. `shut_down` дожидается каждой отправки, увидевшей
  исполнитель работающим, затем осушает очередь жизненного цикла и роняет её (`Receiver` в
  `Option`): принятый `Attach` получает ответ, а не остаётся `Attaching`, поздний получает `Gone`.
- **(a) Тесты:** `a_panic_in_one_slot_rebuilds_the_shared_caches_and_keeps_the_pipelines` (две
  паники в одном ходу, у каждого слота своя, следующий ход рисует оба слота),
  `a_send_racing_the_stop_is_drained_or_handed_back`, `a_command_sent_during_the_stop_comes_back_gone`.
- **(b) Что:** ADR-0305 §8 в движке: `DeviceHealth` контекста с отсрочкой и потолком ADR-0267,
  пересборка устройства на том же instance с сохранением поверхностей, перенастройка каждого
  оконного слота; кадр, проштампованный до пересборки, отклоняется по поколению, как после
  restore; размер в ожидании или отклонённый держит барьер через пересборку; неудачная пересборка
  оставляет типизированную ошибку, которую видит владелец, а не цикл повторов; `RetryDevice`.
- **(b) Тесты:** `a_device_rebuild_keeps_the_surfaces_and_redraws_every_slot_in_full`; тесты
  ADR-0267 и ADR-0244 переписаны на слот.
- **(c) Что:** ось `GpuResourceGeneration` удаляется (§6), раз пересборка у контекста; остановка
  ограничена по времени (`DETACH_DEADLINE`) вместе с потоком растра; замер доли растра в кадре на
  сцене-сетке, записанный в теле PR, как вход для решения о потоке растра по ADR-0305 §10.
- **Ворота:** пункт ADR-0305 в `docs/gaps.md` сведён к строкам «поток растра на платформе X ждёт
  прогона».
- **Зависит:** (a), (b) от RS15a и RS15c; (c) от RS16. **Размер:** L.

## Состояние

| Задача | PR | Состояние | Доказательство |
|---|---|---|---|
| RS1 | #2384 | готова | `painting_it recording_contract::a_non_finite_op_is_refused_by_kind`, `…a_destructor_that_panics_during_reset_leaves_the_canvas_empty` (падает без порядка `reset`); `a_non_finite_op_is_omitted_and_the_frame_draws`, `inline_picture_replay_owns_its_root_clips` на lavapipe; `.snap` не тронуты; `rg draw_picture crates packages` пуст |
| RS3 | #2383 | готова | `layer_it region_contract` (четыре строки), `union_table`, `bounds_round_outward_with_aa_margin`; `damage_readback_tests` на lavapipe без правки ожиданий; `_ =>` над `DamageRegion`, `FullReason`, `ResyncReason` нет |
| RS5 | #2392 | готова | `layer_it scene_apply_contract` (десять строк), `content_version_contract` (две), `scene_table_cost` (четыре): копий 4 / 5 / 5 / 6 на 100 / 1 100 / 30 000 / 100 000 узлах при одних слотах, k = 8 операций в одном листе копируют 2 / 3 / 3 / 4 узла; `scene::table::tests` (модель против `BTreeMap`, `Arc::ptr_eq`, рост 1 024 → 1 025 создаёт 3 узла и 0 копий); `Publish` корня с теми же слотами копирует 3 узла при 40 и при 1 000 детях (до разности слотов 3 и 33); новая граница над 10 и 100 узлами даёт 11 и 101 `depth_updates`; `xtask globals` со строкой `scene::id::NEXT_LINEAGE`, `send-impls` без строк |
| RS14 | #2382 | готова; `RasterExecutor` переходит в RS15 вместе с командами, его единственный API это ход по ним | `every_slot_shares_one_device_one_glyph_atlas_and_one_pipeline_per_format` (lavapipe): 3 слота в двух форматах дают 1 instance, 1 device, 1 атлас, 2 набора пайплайнов, 0 сборок на перерисовке; до: 3/3/3/3 |
| RS6 | #2399 | готова | `layer_it scene_patch_damage` (девятнадцать строк, каждая падает без своей части: следа `Place` с размещением корня, сравнения эффектов, разрешения последователей, выбора присоединённого лидера, глубины цепочки последователей, цепочек до общего предка (12 вложенных последователей: 24 разрешения вместо 531 440), спаривания последователей за один проход (2 000 соседей: 4 001 чтение ссылок вместо 8 006 001), бахромы билинейной выборки лупы, ветви лидера под `LeaderClip::Hide` и последователя, чья цепочка идёт через скрытого, цепочки из 30 000 эффектов на стеке в 256 КиБ, клипа над сжатым фильтром в той же границе и в дочерней, ленивого чтения входов фильтров (50 размытий над 200 кусками: при далёком сдвиге 100 чтений, только записи фильтров, а не 500 с кусками; при правке под ними 600), ядра фильтра под сжимающим предком, суммы ядер вдоль цепочки (три фильтра и лупа между ними; с максимумом вместо суммы строка падает), `damage_extent` картинки, замыкания над подложкой); `patch_damage_covers_diff_damage`: 10 000 случаев по три патча (метки, сдвиги, эффекты, добавление и удаление кусков, перестановки, сдвиг лидера) зелёные за 0,7 с, подсаженный патч без следа одной операции пойман; `the_renderer_kernel_stays_inside_read_closure` (flui-engine); `damage_readback_tests` на lavapipe без правки ожиданий; в тестах differ четыре ожидания следуют правилу §6 (подложка берёт и то, что читает, цветовой фильтр без границы даёт `Full(Unbounded)`); счётчики `followers_resolved`, `follower_pairing_reads`, `filter_input_reads`, `depth_updates`; у differ нет своих `backdrops` и `foreground`, у `frame_protocol.rs` нет своего радиуса: оба зовут `read_closure` и `close_over` |
| RS7 | #2401 | готова | `layer_it pending_log_contract`: пять строк ADR (`a_full_log_folds_its_two_oldest_entries`, `damage_from_an_older_base_covers_the_newer_one`, `a_base_from_another_lineage_renders_full`, `a_lost_completion_over_reports_and_never_under_reports`, `a_surface_change_in_the_log_publishes_full`) и пробы повторного входа `a_released_content_drops_after_the_owner_is_released`, `a_fold_releases_its_contents_after_the_owner_is_released` (40 кадров без подтверждения: журнал не больше 16 записей, свёрнутая запись сохраняет `Full(SurfaceChanged)`); каждая строка падает без своей проверки (линия, «только вперёд», сворачивание, отпускание после заимствования, сброс записей по подтверждению); счётчик `pending_log_folds`; `apply` (в `Applied::released`), `completed` и `renew` возвращают `ReleasedContents` для `Outbox` (ADR-0287), включая заменённую версию и содержимое, отпущенное сворачиванием; память журнала ограничена 16 записями |
| RS15a | #2396 | готова (ревью принято) | `engine_it` 17 тестов; без фикса падают `a_superseded_scene_is_dropped_after_the_inbox_lock_is_released` (взаимоблокировка), `a_full_settlement_ring_holds_the_next_frame`, `a_stopped_executor_settles_every_pending_frame_failed`, `a_failed_resize_is_held_in_the_record_and_retried_next_turn`, `a_panicking_subscriber_cannot_strand_a_refused_slot`, `an_untaken_panic_payload_is_dropped_outside_the_slot_table`, `a_refusal_whose_error_panics_when_formatted_still_commits`, `a_surface_loss_keeps_the_barrier_of_a_pending_resize` |
| RS15c1 | #2407 | готова | `a_surface_targets_display_equals_the_platforms_connection`: X11 под `Xvfb` (`x11_window`), Wayland под `weston` (`platform_it`), `the_headless_display_connection_answers_no_display`; проверка типов Windows, macOS, iOS, Android, wasm32 |
| RS15c2 | #2441 | готова (ревью принято) | `engine_it`: `window_slots::a_window_slot_attaches_unconfigured_and_its_first_size_configures_it`, `…a_frame_stamped_before_a_restore_is_refused_on_its_generation` (падает, если restore не поднимает поколение), `…a_windows_colour_and_latency_reach_the_backend_once_a_turn`, `…a_dropped_surface_is_asked_of_the_owner_and_a_restore_answers_it`, `…a_restore_keeps_the_barrier_of_a_refused_size`, `…a_restore_queued_before_a_surface_loss_does_not_rewind_the_generation`, `raster_executor::a_size_the_backend_always_refuses_is_not_retried_and_warns_once` (падает и при повторе всякого отказа, и при warn на каждый ход), `a_surface_loss_keeps_the_barrier_of_a_refused_size`, `a_failed_resize_is_held_in_the_record_and_retried_next_turn` (строки Timeout и Panic); lib: `surface_factory::tests::a_surface_on_a_foreign_display_is_refused_before_wgpu_sees_it` (три чужих дисплея без сравнения доходят до wgpu), `gpu_context::tests::a_window_its_adapter_cannot_present_to_is_refused_at_attach`, `…a_failed_window_frame_keeps_the_shared_caches` (без фикса второй кадр строит второй атлас), `…a_lost_window_surface_is_left_for_its_owner_to_replace` (исполнитель не создаёт поверхность, владелец получает `SurfaceReplacementNeeded`), `…a_resize_whose_configure_panics_leaves_the_old_size`, `…a_latency_change_repaints_the_next_frame`; compile-fail `surface_factory_is_owner_local`, `surface_factory_rejects_borrowed_window`; поверхности окон без GPU с no-op бэкенда wgpu (`testing::noop_surface_factory`); присутствие на реальном окне проверяет RS16 |
| RS19a | #2445 | готова (ревью принято) | `gpu_context::tests::a_panic_in_one_slot_rebuilds_the_shared_caches_and_keeps_the_pipelines` (lavapipe; паника после записи кадра, до отправки; без сохранения библиотеки после паники наборов пайплайнов 3 вместо 1), `…a_torn_painter_whose_drop_panics_keeps_the_frames_panic` (без сдерживания сброса в записи слота паника сброса вместо паники кадра), `headless::…an_unwound_capture_leaves_the_next_capture_drawing` (наборы пайплайнов не растут), `raster_executor::sender::tests::a_send_racing_the_stop_is_drained_or_handed_back` (без ожидания отправок остановка возвращается раньше отправки), `engine_it raster_executor::a_command_sent_during_the_stop_comes_back_gone` |
| RS4 | #2416 | готова | `rendering_it paint_matrix::a_picture_run_holds_one_render_objects_output` (падает без запечатывания по владельцу); readback `composited_layer_update_readback` и lib flui-engine на lavapipe без правки; снимки paint не перегенерированы; perf: `layers_produced` без изменений во всех сценариях, `slide_transition_tick.layers_reused` 1 → 30 (30 строк панели теперь 30 картинок, а не одна); общие ворота с RS8 на голове производителя: см. строку RS8 |
| RS8 | #2416 (#2427) | готова; update выше 1,6× принят автором спеки, остаток переходит в RS8c | `rendering_it`: `a_clean_boundary_emits_no_operation`, `a_paint_panic_applies_no_patch_and_keeps_the_presented_version`, `a_boundary_holding_a_leader_is_retained`, `a_boundary_painted_alone_reads_the_anchor_its_parent_gave_it`, `retained_frames_match_a_full_repaint_over_random_trees_and_marks` (сравнивает `flatten()`); `flui-runtime`: `a_refused_submit_keeps_its_damage_owed`, `a_refused_patch_rebuilds_under_a_new_lineage`, `a_hot_reload_renews_every_scene_lineage` (две презентации; падает без `renew_scene`); `flui-app`: `a_base_mismatch_renders_full`; `layer_it scene_table_cost::entries_sharing_a_bucket_copy_it_once` (64 записи на одну текстуру и одну ссылку копируют столько же узлов таблицы, сколько одна); readback на lavapipe без правки ожиданий, снимки не перегенерированы. Ворота `paint/rotated_box_turn_change/inline`, одна машина, одинаковая работа кадра: голова = paint + `apply`, main = paint + graft + `LayerDiffer::diff` того же кадра (локальный bench `rotated_box_turn_change_with_diff` на main, не в репозитории). update при N = 1 / 1 000 / 10 000, один прогон criterion (голова, main с differ, снова голова): голова 3.34–3.67 / 5.65 / 19.8–22.6 µs, main с differ 2.02 / 3.51 / 15.4 µs; callgrind update на голове 17.6k / 17.6k / 17.7k Ir (плоско по N; было 21.1k до ссылок без копий матриц, листьев по 8 и узкого unshare). Эквивалентность путей damage: `layer_it scene_effect_update` прогоняет таблицу с общим и с принудительно раздельным путём и сравнивает damage и `flatten()`. Исправления по ревью Codex: `a_replaced_version_drops_outside_the_pipeline_borrow`, `a_pixel_ratio_change_retires_the_nodes_it_replaces`, `a_renewed_owner_numbers_its_scenes_apart`, `a_boundary_under_a_moved_clean_boundary_reads_the_moved_anchor`. repaint при N = 1 000: голова 628 µs, main 541 µs, main с differ 591 µs (≤ 1.5× main). perf baseline: `label_change` 462 (было 502), `list scroll` 4 296 (4 300), `slide_transition_tick` 65 (78), `full_reassemble` 2 873 (2 880) аллокаций; `caret_blink_in_10k_list`: `nodes_painted` и `scene_ops` равны на 1 000 и 10 000; `slide_transition_tick`: `scene_depth_updates` 0, `damage_full` 0 |
| RS8c | | в работе: общий буфер `FragmentOp` на проход (#2447: метка 366 → 298 аллокаций, прокрутка 3 551 → 3 413, перестройка 1 837 → 1 699, рабочие счётчики равны); порядок детей ставится в очередь прохода только для непростого порядка (#2448: кнопка 5 055 → 4 966 живых байт, `boundary_contents::a_parent_painting_its_child_again_drops_the_order_that_left_it_out`); прогон, который только рисует, вклеивается без `Save`/`Restore`, первый прогон оборачивается в своём буфере, пул красок канваса встроен (#2456: repaint `paint/rotated_box_turn_change/inline` при N = 1 000 750 → 620 µs, при 10 000 9.02 → 6.93 ms, update 5.85 → 4.75 µs в одном прогоне criterion; аллокаций repaint при N = 1 000 6 050 → 4 050; `recording::isolated_append_scopes_only_a_run_that_changes_replay_state`); один рекордер и один канвас на проход живут между кадрами, канвас отдаёт запись через `Canvas::take_recording` и держит до четырёх красок без шейдера, слои эффектов узла собираются в буфер прохода, `Matrix4::transform_rect` без деления для аффинной матрицы (repaint при N = 1 000 630 → 504 µs, при 10 000 10.68 → 8.27 ms в одном прогоне criterion с main; callgrind repaint при N = 1 000 4.72M → 3.70M Ir, update 18.3k Ir без изменений; аллокаций: прокрутка 3 060 → 3 018, перестройка 1 388 → 1 346, каретка 60 → 58; `recording::a_taken_recording_shares_its_plain_paints_with_the_next`); освобождённые версии, узлы таблицы и содержимое обновления слоёв возвращаются владельцу сцены (`ReleasedContents::reclaim`, `SceneOwner::restock`): заменённая версия ждёт, пока её не отпустит растр и конвейер, запись копирует общий узел таблицы в пустой запасной, обновление слоёв пишет через `SceneWriter::with_effects` в возвращённое содержимое (`slide_transition_tick` 21 → 17 аллокаций, все 17 в семантике, в paint и сборке сцены ни одной; метка 270 → 262, каретка 58 → 51; `layer_it scene_reclaim`: `reclaimed_buffers_never_change_a_version_someone_holds`, `an_update_writes_into_what_the_owner_reclaimed`; callgrind update при N = 1 / 1 000 / 10 000 после 20 показанных кадров 17.6k / 17.7k / 17.7k Ir без единой аллокации против 16.7k Ir с четырьмя на голове предыдущего шага: возврат и разбор запасов стоят около 1k Ir, это плата за кадр без кучи); дальше буферы границ и патча; следующий PR линии после RS8 и до любой задачи, добавляющей работу в `apply` или `refresh_boundary`; цели repaint ≤ 478 µs при N = 1 000 и update 2.13 / 3.08 / 12.7 µs при N = 1 / 1 000 / 10 000 | update на голове RS8 в одном прогоне с main с differ: 3.34–3.67 против 2.02 µs, 5.65 против 3.51 µs, 19.8–22.6 против 15.4 µs; разбивка callgrind на update при N = 1 000: `apply` 10.6k Ir (`patch_damage` 8.0k: замыкание `effects_damage` 1.9k, `Scope::through` 1.6k, `mapped` 1.5k, `Geometry::fold` 1.2k, `node_links` 0.9k), `run_paint` 5.6k (`refresh_boundary` 2.9k, `scene_applied` 1.2k), `memcpy` 2.3k, в основном переносы `BoundaryRecord`. repaint при N = 1 000 на голове 504 µs против 630 µs main той же машины (цель 478 µs мерилась на машине, где main давал 541 µs); разбивка callgrind 3.70M Ir: `malloc` 16 % (три аллокации на лист: команды канваса, `Arc<DisplayList>` и `Arc<Chunk>`; два `Arc` сливаются, когда RS9 уберёт `PictureLayer::from_shared`), `memcpy` 16 % (переносы `DrawCommand` в `DisplayList::record`, `Chunk::new`, `seal_picture`), `paint_subtree_impl` 14 %, `Matrix4::transform_rect` 5 %. Известная цена с RS8: ошибка семантики после paint отбрасывает патч, и повтор кадра перерисовывает корневую границу целиком, а не только границы, которые кадр был должен |
| RS19b | #2477 | готова | `engine_it raster_executor`: `a_device_rebuild_refuses_the_frames_stamped_before_it_and_redraws_in_full` и `…a_failed_rebuild_is_reported_and_the_next_waits_out_its_backoff` (без перештамповки после пересборки кадр до неё рисуется), `…a_device_rebuild_keeps_the_barrier_of_a_refused_size` (без барьера размера кадр под отклонённый размер рисуется), `…a_given_up_rebuild_holds_until_the_owner_sends_retry_device`, `…a_rebuild_that_panics_with_its_device_in_place_restamps_every_slot` (без перештамповки кадр до потери рисуется), `…a_loss_met_by_a_frame_is_published_with_its_rebuild_due_now` (без публикации после обхода слотов здоровье остаётся `Healthy`), `…a_backend_that_cannot_rebuild_inline_gives_up_at_once`, `…an_offscreen_slot_the_rebuilt_device_refuses_holds_why_and_asks_no_surface` (с `wants_surface` для любого слота падает), `…a_slow_failed_rebuild_waits_its_backoff_from_when_it_ended` (без повторного чтения часов ожидание 0 вместо 16 мс), `…a_panicking_clock_still_publishes_a_loss_and_wakes_the_owner` (без защиты чтения часов в `note_loss` потеря не публикуется), `window_slots::a_window_the_rebuilt_device_cannot_present_to_asks_for_a_new_surface`, `…a_window_over_the_rebuilt_device_limit_waits_for_a_size_that_fits` (с `wants_surface` для любого отказа окна падает), `…a_rebuild_refusal_is_committed_before_the_clock_is_read_again` (с чтением часов сразу после попытки отказ теряется); `gpu_context::tests::a_refusal_outlives_the_attempt_that_found_it` (без переноса отказов между попытками отказ теряется); `device_health::tests::the_rebuild_backoff_climbs_to_its_ceiling_and_gives_up_after_ten_at_it` (без ожидания отсрочки тест отсрочки падает); `gpu_context::tests::a_device_rebuild_keeps_the_surfaces_and_redraws_every_slot_in_full` (lavapipe; без сброса художника кадры на новом устройстве остаются `DeviceLost`). Окна на новом устройстве без GPU не проверены: lavapipe без окна, у no-op адаптера нет поверхности |
| RS9 | #2504, #2525 | готова; слои читаются из версии на месте, `flatten` за `testing`; счётчик обхода движка уходит в RS12 вместе со счётчиками записи | `flui-layer`: `layer_it::scene_links::a_version_resolves_links_and_annotations_as_its_flattened_tree_does` (5 000 случайных версий: снимок слоёв версии читает те же виды в том же порядке, что плоское дерево), `layer_it::inspect::*` (поля слоёв через версию); `flui-testing`: `inspect::a_layer_id_names_its_boundary_while_it_stays` (кадр без изменений сохраняет все id, корень переживает перерисовку; id другого поколения, другой линии и записи за концом — `gone`; падает, если не сверять линию); `flui-widgets` perf: `perf_an_idle_frame_leaves_the_retained_scene_alone` (G3 через `HeadlessHost` и `UiRuntime::pump`: 4 запрошенных кадра не публикуют версию и ничего не отправляют, ни одной аллокации в `flui-layer`; остальные 6 аллокаций кадра — буферы выборки runtime, эпоха и когорта post-frame, ответы text sizing, запрос пробуждения — держит базовая линия `xtask perf`); `rg 'flatten\(' crates --glob '!**/tests/**'` находит только определение за `testing`; `flui-engine` lib: `layer_walk::tests::a_version_walks_as_its_flattened_tree` (2 000 случайных версий с эффектами, лидерами и последователями: тот же порядок, те же слои, куски и размещения, последователи с тем же преобразованием до 1e-6), `…a_partial_walk_enters_only_the_boundaries_its_damage_reaches` (ряд из 10 и из 100 границ: частичный обход входит в 3 узла в обоих; падает, если отсечение снять), `version_readback_tests::a_partial_frame_of_a_version_draws_as_its_flattened_tree` (lavapipe: частичный кадр версии пиксель в пиксель с плоским деревом, страж вне damage цел; падает, если отсекать все слоты), `…a_raster_panic_renders_the_next_frame_in_full`, `…a_boundary_in_the_last_partly_damaged_pixel_draws_its_share` (damage кончается внутри пикселя: граница в нём рисует свою долю; падает при отсечении по сырому damage), `…a_clipped_destination_reading_chunk_redraws_inside_its_clip` (multiply под клипом: кадр как у плоского дерева, следующий кадр частичный; падает, если проверять straddle и для версии), `layer_walk::tests::a_culled_walk_enters_every_chunk_its_damage_meets` (2 000 случайных версий и damage: каждый кусок, который плоское дерево сажает в damage, обход посещает; оракул не читает `Geometry`; падает без размещения в цепочке), `…a_culled_walk_enters_a_slot_whose_blur_reaches_the_damage` (падает без ядра фильтра), `layer_it::a_cull_asked_in_walk_order_answers_as_one_asked_alone`, `engine_it::retained_version::a_retained_version_renders_as_its_flattened_tree` (публичный `HeadlessRenderer::render_scene`; падает, если обход версии не рисует куски); `engine_it`, `layer_it`, `app_it`, `flui-runtime` зелёные; `raster_owner::tests::superseded_damage_folds_into_the_survivor`, `damage_readback_tests::a_moved_box_repaints_its_old_and_new_positions_only` на месте |
| RS2, RS10–RS13, RS15b, RS16–RS18, RS19c | | не начата | |
