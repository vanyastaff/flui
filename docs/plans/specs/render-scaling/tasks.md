# render-scaling: задачи

- **Статус:** утверждена 2026-11-11; исполнение. Сделано: RS1, RS3, RS5, RS6, RS7, RS14, RS15a, RS15c1; RS15c2 в ревью. Следующие: RS8 с RS4 и RS16, затем RS15b (RS8 и RS16 не
  зависят друг от друга); RS4 сливается в одной серии с RS8.
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
- **Зависит:** RS4 (в той же серии), RS7. **Размер:** XL. **Ревью сильной модели.** Если PR выходит за 3 000 строк
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

- **Что:** ADR-0305 §7–§9: паника сдерживается на команду и стоит общих кэшей, но не пайплайнов;
  потерянное устройство пересобирается с сохранением поверхностей; остановка ограничена по времени;
  ось `GpuResourceGeneration` удаляется (§6), раз пересборка у контекста.
  `shut_down` закрывает очередь жизненного цикла (`Receiver` в `Option`), чтобы
  `Attach`, прошедший проверку health до остановки, получил `Gone`, а не остался `Attaching`.
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
| RS3 | #2383 | готова | `layer_it region_contract` (четыре строки), `union_table`, `bounds_round_outward_with_aa_margin`; `damage_readback_tests` на lavapipe без правки ожиданий; `_ =>` над `DamageRegion`, `FullReason`, `ResyncReason` нет |
| RS5 | #2392 | готова | `layer_it scene_apply_contract` (десять строк), `content_version_contract` (две), `scene_table_cost` (четыре): копий 4 / 5 / 5 / 6 на 100 / 1 100 / 30 000 / 100 000 узлах при одних слотах, k = 8 операций в одном листе копируют 2 / 3 / 3 / 4 узла; `scene::table::tests` (модель против `BTreeMap`, `Arc::ptr_eq`, рост 1 024 → 1 025 создаёт 3 узла и 0 копий); `Publish` корня с теми же слотами копирует 3 узла при 40 и при 1 000 детях (до разности слотов 3 и 33); новая граница над 10 и 100 узлами даёт 11 и 101 `depth_updates`; `xtask globals` со строкой `scene::id::NEXT_LINEAGE`, `send-impls` без строк |
| RS14 | #2382 | готова; `RasterExecutor` переходит в RS15 вместе с командами, его единственный API это ход по ним | `every_slot_shares_one_device_one_glyph_atlas_and_one_pipeline_per_format` (lavapipe): 3 слота в двух форматах дают 1 instance, 1 device, 1 атлас, 2 набора пайплайнов, 0 сборок на перерисовке; до: 3/3/3/3 |
| RS6 | #2399 | готова | `layer_it scene_patch_damage` (девятнадцать строк, каждая падает без своей части: следа `Place` с размещением корня, сравнения эффектов, разрешения последователей, выбора присоединённого лидера, глубины цепочки последователей, цепочек до общего предка (12 вложенных последователей: 24 разрешения вместо 531 440), спаривания последователей за один проход (2 000 соседей: 4 001 чтение ссылок вместо 8 006 001), бахромы билинейной выборки лупы, ветви лидера под `LeaderClip::Hide` и последователя, чья цепочка идёт через скрытого, цепочки из 30 000 эффектов на стеке в 256 КиБ, клипа над сжатым фильтром в той же границе и в дочерней, ленивого чтения входов фильтров (50 размытий над 200 кусками: при далёком сдвиге 100 чтений, только записи фильтров, а не 500 с кусками; при правке под ними 600), ядра фильтра под сжимающим предком, суммы ядер вдоль цепочки (три фильтра и лупа между ними; с максимумом вместо суммы строка падает), `damage_extent` картинки, замыкания над подложкой); `patch_damage_covers_diff_damage`: 10 000 случаев по три патча (метки, сдвиги, эффекты, добавление и удаление кусков, перестановки, сдвиг лидера) зелёные за 0,7 с, подсаженный патч без следа одной операции пойман; `the_renderer_kernel_stays_inside_read_closure` (flui-engine); `damage_readback_tests` на lavapipe без правки ожиданий; в тестах differ четыре ожидания следуют правилу §6 (подложка берёт и то, что читает, цветовой фильтр без границы даёт `Full(Unbounded)`); счётчики `followers_resolved`, `follower_pairing_reads`, `filter_input_reads`, `depth_updates`; у differ нет своих `backdrops` и `foreground`, у `frame_protocol.rs` нет своего радиуса: оба зовут `read_closure` и `close_over` |
| RS7 | #2401 | готова | `layer_it pending_log_contract`: пять строк ADR (`a_full_log_folds_its_two_oldest_entries`, `damage_from_an_older_base_covers_the_newer_one`, `a_base_from_another_lineage_renders_full`, `a_lost_completion_over_reports_and_never_under_reports`, `a_surface_change_in_the_log_publishes_full`) и пробы повторного входа `a_released_content_drops_after_the_owner_is_released`, `a_fold_releases_its_contents_after_the_owner_is_released` (40 кадров без подтверждения: журнал не больше 16 записей, свёрнутая запись сохраняет `Full(SurfaceChanged)`); каждая строка падает без своей проверки (линия, «только вперёд», сворачивание, отпускание после заимствования, сброс записей по подтверждению); счётчик `pending_log_folds`; `apply` (в `Applied::released`), `completed` и `renew` возвращают `ReleasedContents` для `Outbox` (ADR-0287), включая заменённую версию и содержимое, отпущенное сворачиванием; память журнала ограничена 16 записями |
| RS15a | #2396 | готова (ревью принято) | `engine_it` 17 тестов; без фикса падают `a_superseded_scene_is_dropped_after_the_inbox_lock_is_released` (взаимоблокировка), `a_full_settlement_ring_holds_the_next_frame`, `a_stopped_executor_settles_every_pending_frame_failed`, `a_failed_resize_is_held_in_the_record_and_retried_next_turn`, `a_panicking_subscriber_cannot_strand_a_refused_slot`, `an_untaken_panic_payload_is_dropped_outside_the_slot_table`, `a_refusal_whose_error_panics_when_formatted_still_commits`, `a_surface_loss_keeps_the_barrier_of_a_pending_resize` |
| RS15c1 | #2407 | готова | `a_surface_targets_display_equals_the_platforms_connection`: X11 под `Xvfb` (`x11_window`), Wayland под `weston` (`platform_it`), `the_headless_display_connection_answers_no_display`; проверка типов Windows, macOS, iOS, Android, wasm32 |
| RS15c2 | | в ревью | `engine_it`: `window_slots::a_window_slot_attaches_unconfigured_and_its_first_size_configures_it`, `…a_frame_stamped_before_a_restore_is_refused_on_its_generation` (падает, если restore не поднимает поколение), `…a_windows_colour_and_latency_reach_the_backend_once_a_turn`, `raster_executor::a_size_the_backend_always_refuses_is_not_retried_and_warns_once` (падает и при повторе всякого отказа, и при warn на каждый ход), `a_failed_resize_is_held_in_the_record_and_retried_next_turn` (строки Timeout и Panic); lib: `surface_factory::tests::a_surface_on_a_foreign_display_is_refused_before_wgpu_sees_it` (три чужих дисплея без сравнения доходят до wgpu), `gpu_context::tests::a_window_its_adapter_cannot_present_to_is_refused_at_attach`; compile-fail `surface_factory_is_owner_local`, `surface_factory_rejects_borrowed_window`; поверхности окон без GPU с no-op бэкенда wgpu (`testing::noop_surface_factory`); присутствие на реальном окне проверяет RS16 |
| RS2, RS4, RS8–RS13, RS15b, RS16–RS19 | | не начата | |
