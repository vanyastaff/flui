# Foreground-фильтры: bounded footprint и signed attachment mapping

Реализация сохраняет прямой wgpu-путь. Зависимости, backend abstraction, публичные constructor API и snapshots не изменены. Исходный аудит: [карта конвейера и сравнение контрактов](2026-10-04-engine-skia-anyrender-audit.ru.md).

## Ревизии и изоляция

Исследование и начало реализации: FLUI `dca90bac047e950b85943b617cccf460f723f894`. Перед финальными gates ветка fast-forward на актуальный main `2992d8c4444715587c23dee706c35eb4b8cfbb03`, содержащий #1420. Собственные изменения восстановлены поверх него; renderer.rs и чужие изменения не редактировались. #1421 уже находился в исходной базе; открытый #1422 и соседняя сессия glyph validation не пересекаются с исправлением.

Upstream-код использован только как исследовательский источник: Skia `8643b1d64cff21b5e6f8d65ca98204c6eecb0098`, AnyRender `870407d142a2cd38ea6404717c6d68dceeeb91d6`; сборка Skia не запускалась.

## Контракт границ

Все перечисленные области находятся в root device pixels:

- Source support сохраняет консервативную геометрию до clipping. AA-поля primitive shaders включаются до integer cover; для glyph/image/gradient/shadow и ordered groups без надёжного AABB используется неизвестная поддержка, а не ошибочный неполный AABB.
- Desired output ограничивается viewport, входной потребностью enclosing filters и захваченным composite clip.
- Required input получается обратным расширением desired output по сумме фактических per-axis радиусов: Blur `ceil(sigma * 1.7320508)`, Morph `ceil(radius)`. Эти значения совпадают с FLUI shaders; коэффициенты Skia не переносились.
- Admitted input — source support, пересечённый с required input. Unknown source и матрица, создающая alpha из прозрачных пикселей, используют ограниченный required input.
- Attachment покрывает пересечение консервативного forward support и required input. Signed origin округляется вниз до чётного device pixel для сохранения derivative quad lattice; far edge округляется вверх.
- После каждого прохода Blur/Morph промежуточная поддержка расширяется по этому проходу и ограничивается attachment. ColorMatrix с положительным alpha offset включает весь attachment. Следующий проход читает эту поддержку.
- Output support и final composite clip используются отдельно от input. UV рассчитываются из того же integer texel grid, поэтому crop не сдвигает дробную геометрию.

Размеры ограничены device limits, конечностью/представимостью координат и существующими quota для prepared ресурсов и sampling work. Дальний или огромный source AABB не задаёт размер offscreen. Invalid sigma/radius даёт typed geometry error: negative/nonfinite, непредставимое сужение в f32, непригодный для shader arithmetic sigma² и слишком большой loop radius. Нулевая ось — identity этой оси.

## Единый remap и ownership

Immutable viewport binding несёт signed attachment origin. Все обслуживаемые primitive vertex paths вычитают его при расчёте NDC; geometry, gradient coordinates, UV и analytic membership остаются root-space. Scissors остаются signed до окончательного пересечения с attachment. Masks включают тот же origin в attachment-to-root mapping. Nested opacity/filter replay, SSAA tiles, advanced backdrop copies и существующее чтение backdrop используют согласованный parent mapping.

Remap не клонирует scene geometry. Footprint читается из borrowed passes; записанная цепочка перемещается в FilterOp без дополнительной heap-копии. Старый тест, ожидавший отказа именно из-за удалённой копии, теперь проверяет видимую отрисовку при прежнем лимите 1 MiB / 3 elements. Реальная одновременно живая SSAA-копия имеет отдельный refusal/recovery case. Порог или бюджет не увеличен.

Перед texture acquisition резервируются размеры, работа, destination storage, uniforms и nested targets/composites в затронутом foreground domain. Permits следуют существующему submit/completion ownership. На Result-отказе восстанавливается parent attachment; следующая граница кадра дополнительно сбрасывает origin/depth/viewport после возможного containment unwind. Общий resident VRAM budget этим изменением не заявляется.

## Постоянные regression tests

`blur_filter_tests/foreground.rs` включён в существующий test binary и последовательную группу `gpu-readback` nextest:

- `foreground_filter_viewport_crop_contract`: малый viewport 32×32 сравнивается с crop (32,32)..(64,64) большой отрисовки 96×96. Direct painter, recorded Canvas/SceneRenderer и их взаимное сравнение. Все края/углы, anisotropic sigma, дробное смещение, scale, clips/masks/layers, Compose/Morph, zero axes и удалённые источники; обслуживаемые shapes, glyphs, textures и gradients, portable Plus и advanced Multiply.
- `foreground_filter_chains_match_independent_nested_layers`: Compose сравнивается с отдельно вложенными фильтрами. Этот oracle обнаруживает потерю intermediate halo даже если оба размера сцены ошибаются одинаково.
- Invalid parameter и prepared quota families проверяют отказ до и после переключения attachment, отказ sampling work и следующий здоровый кадр в том же painter/renderer.

Допуск — один UNORM8 channel unit, учитывающий rounding промежуточных H/V targets и финального composite. Он не увеличен после результатов. Нулевые фильтры совпадают с unfiltered побайтно. Отдельные assertions требуют видимых glyph/circle pixels и белого отрицательного контроля.

[Counterfactual runner](verify-foreground-filter-counterfactuals.py) возвращает каждый дефект отдельно, выполняет настоящий GPU-test и восстанавливает точные исходные bytes в finally. Все четыре восстановленных дефекта дали exit 101 / `test result: FAILED` по пикселям, без compile failure:

| Возвращённый дефект | Отличающийся test |
| --- | --- |
| Source intersect viewport до расширения | Crop: края/углы и primitive paths |
| Original support повторно используется каждым проходом | Independent chain: two blurs, anisotropic chain, Blur→Dilate, Blur→Erode |
| Glyphs исключены из mixed source bound | Только rect_and_overflowing_glyph |
| Circle bound не учитывает shader minimum radius | Только tiny_circle_large_scale |

См. `foreground-filter-counterfactual-*.log`. Runner и измерительный инструмент меняют временные исходники: запускать последовательно, без других build workers в этом worktree. Перед мутацией они сохраняют точные bytes на диск в отдельной исследовательской директории и повторяют восстановление при временной Windows IO-ошибке. Финальный измерительный прогон восстановил каждый файл побайтно; instrumentation в production отсутствует.

## Измерения

[Воспроизводимый runner](measure-foreground-filter.py), raw log (локальный `foreground-filter-measurement.log`), [samples](foreground-filter-measurements.json). Windows, NVIDIA GeForce RTX 3070 Ti, DX12, driver 32.0.15.6094, wgpu 30.0.1, Rgba8Unorm. Viewport 32×32; каждая сцена использует свежий painter, 3 warm-up и 10 samples. Wall time включает target creation/clear, encoding, submit и GPU readback. Счётчик проходов охватывает engine rendering, исключая target clear/readback; измерительная печать включена в timing. Это не GPU timestamps и не обычный presentation frame time.

| Сцена | Offscreen dimensions | Engine passes | Pool allocation high-water, bytes | min / median / max, ms |
| --- | --- | ---: | ---: | --- |
| Источник слева, sigma=(4,4) | 16×24, три textures | 5 | 4608 | 0.733 / 0.837 / 0.871 |
| Два Blur | 30×39, три textures | 7 | 14040 | 0.825 / 0.890 / 0.984 |
| Nested filter | 47×47 и 14×15 | 9 | 29028 | 1.194 / 1.275 / 1.433 |
| Source покрывает ±10⁸, sigma=(4,4) | 47×47, три textures | 5 | 26508 | 0.695 / 0.749 / 0.880 |
| Source на удалении 10⁸ | Нет offscreen | 0 | 0 | 0.403 / 0.414 / 0.426 |

Pool high-water — измеренные выделенные texel bytes pool, включая idle returns; это не точный пик всех живых GPU ресурсов или физической VRAM. Driver retention, staging, buffers, masks и atlases не входят. Сравнение с исходным аудитом не является isolated benchmark: разные сцены/работа и instrumentation. Производительность не объявляется улучшенной только по этим wall times.

## Проверки и ограничения

На финальном коде и main 2992d8c44 полный engine GPU binary: **63 passed, 0 failed, 0 ignored** (166.02 s), FLUI_REQUIRE_GPU=1, testing, один test thread; raw log (локальный `foreground-filter-engine-tests-main.log`). Оба independent oracle, четыре counterfactual и восстановление после отказа проверены на этом коде. Strict engine clippy с testing/all-targets прошёл. Первый широкий check-changed: 651 passed, 10 штатных ignored, strict rustdoc/doctests и доступные cross-typechecks прошли; полный raw log (локальный `foreground-filter-check-changed.log.gz`), gzip сохраняет исходные bytes. Финальный повтор после устранения pass-chain копий: **cargo xtask checks PASS**, **cargo xtask check-changed PASS**; 651 passed, 10 штатных ignored (80.474 s). Checks (локальный `foreground-filter-checks.log`), check-changed (локальный `foreground-filter-check-changed-final.log`), strict clippy (локальный `foreground-filter-clippy.log`). nextest show-config (локальный `foreground-filter-nextest-group.log`) подтвердил все четыре семейства в gpu-readback с max-threads=1.

Проверено headless GPU readback на Windows/DX12. Широкий check-changed включает strict rustdoc, doctests, Win32 platform/CLI/MCP и AppKit platform/MCP typecheck, а также wasm workspace/facade typecheck; отдельные результаты и host skips приведены в raw log. Это не execution engine на этих других targets. 10 штатных ignored-тестов workspace относятся к интерактивному desktop MCP и helper-процессам. Native window presentation, Linux/Vulkan/GLES, macOS/Metal, Android/iOS и wasm/WebGPU execution не проверены. Exact whole-GPU live peak, physical VRAM и GPU timestamps не измерены. Новые backdrop операции и affine filter semantics не добавлены.

## Отдельные следующие задачи

Остаются задачи исходного аудита: vertex alpha, Solid shader color, textured meshes и их sampler/UV/color contract; полный resident/in-flight GPU accounting; capability contract supplied wgpu device и отдельное решение constructor API; snapping/atlas stress. Они не включены в этот diff. Наличие аналогичной возможности в Skia не делает её обязательной для FLUI.

Сырые логи запусков остаются локальными и исключены через `.gitignore`. Проверяемые результаты приведены выше; скрипты воспроизведения и JSON samples включены в репозиторий.
