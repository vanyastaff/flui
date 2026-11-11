# render-scaling: задачи

- **Статус:** утверждена 2026-11-11; исполнение. Сделано: RS1, RS14. Следующие: RS3, RS4, RS5,
  RS15 (не зависят друг от друга).
- **Design:** [design.md](design.md); требования и ворота G1–G7: [requirements.md](requirements.md)
- **База:** `main` @ `da69efa`. Пути и строки в ADR даны на `e5ba8fe8f`; перед правкой сверить.

## Правила исполнения

- Задача = ветка = PR, проверки по `AGENTS.md`, «Checking a change» и «Before each push».
- PR не добавляет ADR. Если шаг не строится без решения, которого в ADR-0304, ADR-0305 и ADR-0306
  нет, исполнитель останавливается и присылает вопрос координатору.
- Тест задачи падает при откате её производственной правки. Имена тестов взяты из разделов
  Confirmation трёх ADR; менять имя можно только вместе с ADR.
- Счётчик, названный в задаче, добавляется в `FrameReport::counters()` и в
  `crates/flui-widgets/perf/baseline.toml` тем же PR; изменение чужих строк baseline объясняется в
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
  render-объекта; эффект хранит `RenderId` того, кто его толкнул. Работает в сегодняшнем
  `LayerTree`.
- **Где:** `crates/flui-rendering/src/pipeline/owner/paint.rs` (слияние прогонов).
- **Тесты:** `a_picture_run_holds_one_render_objects_output` (новый, `flui-rendering/tests`).
- **Ворота:** readback без изменений; рост `layers_produced` в baseline объяснён построчно; снимки
  paint перегенерируются только в границах прогонов.
- **Зависит:** нет. **Размер:** M.

### RS5. Персистентная таблица, `SceneVersion`, `apply` с проверкой

- **Что:** ADR-0304 §1–§3: таблица с копированием пути (форма в [design.md](design.md)),
  `SceneVersion`, `SceneNodeId`, `BoundaryContent`, `Entry`, `Chunk`, `ContentVersion`, `Lineage`,
  `Placement::try_new`, `SceneOp`, `SceneWriter`, `SceneOwner::apply` с проверкой «всё или ничего»
  и `SceneError`; три индекса (ссылки, пользователи текстур, узлы с фильтрами);
  `SceneVersion::flatten()` за фичей `testing`. Damage в этой задаче всегда `Full(FirstFrame)`.
- **Где:** новые модули `crates/flui-layer/src/scene/`; `Cargo.toml` (`globals` для `Lineage`).
- **Тесты:** модельная проверка таблицы против `BTreeMap` и разделение поддеревьев по
  `Arc::ptr_eq`; из `scene_apply_contract`: `a_slot_naming_its_own_ancestor_is_refused`,
  `a_stale_generation_is_refused`, `a_node_named_by_two_slots_is_refused`,
  `a_refused_patch_leaves_the_version_and_log_unchanged`; из `content_version_contract`:
  `a_replaced_owner_never_reports_unchanged`,
  `an_exhausted_mint_renews_the_lineage_and_publishes_full`.
- **Ворота:** `apply` патча из k операций на версии из 100 и из 100 000 узлов копирует одинаковое
  число узлов таблицы (счётчик в тесте); `cargo xtask globals` и `send-impls` без новых строк.
- **Зависит:** нет. **Размер:** L. **Ревью сильной модели.**

### RS6. Damage как след патча, `read_closure`, последователи

- **Что:** ADR-0304 §4 и §6: след `Publish`, `Place`, `Retire`; `subtree_bounds` с ростом вверх;
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
  захватов ADR-0253 §2, граница с `Leader` или `Follower` удерживается; `FrameSink::submit`
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
  10 000 элементов; `damage_rects`, `damage_full`, `scene_nodes_visited` введены; readback-наборы
  без правки ожиданий.
- **Зависит:** RS4, RS7. **Размер:** XL. **Ревью сильной модели.** Если PR выходит за 3 000 строк
  диффа, первым отдельным PR уходит `FrameSink::submit(ScenePublication)` с публикацией
  `Full` на каждый кадр, вторым производитель патчей.

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
  парой `(Lineage, SceneNodeId)`.
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

- **Что:** ADR-0305 §2–§6: `SurfaceFactory` (`!Send`), закрытые команды слота, расчёт кадра по
  штампу, темп и `SurfaceGeneration` на слот.
- **Тесты:** compile-fail `surface_factory_is_owner_local`, `surface_factory_rejects_borrowed_window`;
  `a_full_lifecycle_queue_returns_the_command`, `a_stale_slot_id_is_refused_after_detach`,
  `a_superseded_scene_is_dropped_after_the_inbox_lock_is_released`,
  `a_full_settlement_ring_holds_the_next_frame`,
  `a_stopped_executor_settles_every_pending_frame_failed`;
  `a_queued_frame_settles_by_its_stamp_and_a_stale_settlement_is_ignored` (`flui-runtime`);
  протокольные тесты `raster_owner` переписаны на слот (список в ADR-0305, Confirmation).
- **Ворота:** все перечисленные тесты; ни одного `Mutex` в типах слота и исполнителя.
- **Зависит:** RS14. **Размер:** L. **Ревью сильной модели.**

### RS16. Настольные раннеры на слотах

- **Что:** Win32, AppKit, Wayland и X11 раннеры `flui-app` работают через слоты; удаляются
  `with_backend`, `Arc<Mutex<RasterLane>>` и замыкание-хук; у окна нет своего `Renderer`.
- **Тесты:** `a_surface_targets_display_equals_the_platforms_connection` (строки Wayland под
  `weston`, X11 под `Xvfb`, headless); `a_hidden_slot_is_not_acquired_and_settles_not_shown`.
- **Ворота:** `rg 'Arc<Mutex<RasterLane>>|with_backend' crates` пуст; два окна примера делят один
  device (G5 через оконный путь на Linux CI).
- **Зависит:** RS15, RS9. **Размер:** L. Windows и macOS: проверка типов и ночной раннер.

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

- **Что:** ADR-0305 §7–§9: паника сдерживается на команду и стоит общих кэшей, но не пайплайнов;
  потерянное устройство пересобирается с сохранением поверхностей; остановка ограничена по времени.
  Плюс замер: доля растра в кадре на сцене-сетке, записанная в теле PR, как вход для решения о
  потоке растра по ADR-0305 §10.
- **Тесты:** `a_panic_in_one_slot_rebuilds_the_shared_caches_and_keeps_the_pipelines`,
  `a_device_rebuild_keeps_the_surfaces_and_redraws_every_slot_in_full`; тесты ADR-0267 и ADR-0244
  переписаны на слот.
- **Ворота:** пункт ADR-0305 в `docs/gaps.md` сведён к строкам «поток растра на платформе X ждёт
  прогона».
- **Зависит:** RS15. **Размер:** L.

## Состояние

| Задача | PR | Состояние | Доказательство |
|---|---|---|---|
| RS1 | #2384 | готова | `painting_it recording_contract::a_non_finite_op_is_refused_by_kind`, `…a_destructor_that_panics_during_reset_leaves_the_canvas_empty` (падает без порядка `reset`); `a_non_finite_op_is_omitted_and_the_frame_draws`, `inline_picture_replay_owns_its_root_clips` на lavapipe; `.snap` не тронуты; `rg draw_picture crates packages` пуст |
| RS14 | #2382 | готова; `RasterExecutor` переходит в RS15 вместе с командами, его единственный API это ход по ним | `every_slot_shares_one_device_one_glyph_atlas_and_one_pipeline_per_format` (lavapipe): 3 слота в двух форматах дают 1 instance, 1 device, 1 атлас, 2 набора пайплайнов, 0 сборок на перерисовке; до: 3/3/3/3 |
| RS2–RS13, RS15–RS19 | | не начата | |
