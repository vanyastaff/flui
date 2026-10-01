# Аудит возможностей flui-engine

## Границы и метод

Проверяется реализация UI-растеризации на wgpu: lowering DisplayList, порядок
рисования, клипы, композиция слоёв, фильтры, текст, изображения, ресурсы,
damage и жизненный цикл поверхности. Skia/Impeller — полезный список задач
растеризатора, но соответствие им не является контрактом FLUI и в этом аудите
не измерялось. Замена wgpu или добавление альтернативного бэкенда не предлагается.

Полнота match по DrawOp доказывает наличие обработчика, а не правильность
пикселей. Приоритет имеют readback с независимым ожидаемым результатом и тест,
который падает при возврате ошибочного production-кода. PNG полезен для
визуального осмотра, но сам по себе не является проверенным golden-эталоном.

## Карта возможностей

| Семейство | Реализованный путь | Ограничения и необходимые проверки |
|---|---|---|
| Rect, RRect, Circle, Oval, Line, Arc, DRRect, Path | Инстансы и lyon-тесселяция | Сравнивать пересечения примитивов, stroke, fill rules, отражение, skew и дробный DPR; наличие пути не доказывает каждый вариант Paint |
| Points, Vertices | Lowering в batches | Отдельно проверять vertex colours, UV и topology; никаких обещаний поддержки по одной сигнатуре |
| Color, Paint | Полноэкранные заливки | Заливка под CTM, clip, blend и save-layer должна иметь один контракт |
| Linear, Radial, Sweep gradients | WGSL и immutable таблица stops | Ordered runs и девятый stop проверены readback; coordinate/tile-mode contract остаётся неполным |
| Image, Repeat, NineSlice, Filtered, Atlas | Кеш, атлас, texture instances | Аффинное преобразование image quad и Porter–Duff режимы реализованы неполно; FilterQuality не всегда учитывается |
| Texture / TextureLayer | Реестр внешних GPU-текстур | Нужны production-владелец регистрации и обновления, проверка формата, размера и usage; readback не заменяет video/platform smoke |
| Paragraph | Глифы подготовленного ShapedParagraph, собственный atlas | Шейпинг принадлежит painting; есть проверки glyph colour, fallback, RTL, baseline, rotation. Нужны стабильные bundled fonts для переносимых golden |
| ClipRect | Scissor | Rotation/skew/reflection дают bounding box, а не точный повёрнутый clip |
| ClipRRect / ClipSuperellipse | Scissor + SDF | Один активный SDF не выражает пересечение нескольких фигур; вложенные углы требуют самостоятельного теста |
| ClipPath | Bounding box пути | Контент вне фигуры, но внутри bounds, проходит; это документированное приближение, не точный clip |
| ClipOp::Difference | Отказ с диагностикой | Вырезание области не реализовано; существующий тест проверяет отказ, не геометрию difference |
| Save/Restore, Offset, Transform | Стек состояния | Проверять siblings после вложенного эффекта, empty clip и отражения; perspective требует отдельного решения о поддерживаемом пространстве |
| Opacity, SaveLayer | Изолированный offscreen | Нужны таблицы перекрывающихся children, nested blend и clip; простое умножение alpha каждого draw не заменяет group opacity |
| ColorFilter, ImageFilter | GPU passes и filter chains | Вложенный порядок команд должен сохраняться целиком; анизотропный blur и преобразование sigma требуют дополнительных проверок |
| BackdropFilter | Flush, backdrop copy, blur, composite | Windowed path поддерживает blur; другие ImageFilter пропускаются. Capture headless не применяет backdrop |
| ShaderMask | Capture children, shader, composite | Headless и вложенный dispatcher без offscreen пропускают реальную маску; nested blend отличается от внешнего replay |
| Leader/Follower | Разрешение связи в windowed visitor | Headless не разрешает leader offset; требуется один scene visitor |
| PlatformView | Контракт слоя и lowering | Настоящая host composition должна проверяться с платформенным владельцем, а не по наличию enum |
| AnnotatedRegion | Метаданные | Не должна рисовать; не относится к возможностям растеризатора |
| PerformanceOverlay | Заливка и подготовленные текстовые labels | Есть readback; это не измерение frame pacing или GPU latency |
| Partial damage | Retained target, scissored clear, full blit | Известен transient advanced-shape straddle; следующий full frame не делает текущий кадр правильным |
| Surface recovery | Owned target, lease, bounded acquisition retry | Fake tests проверяют протокол; minimize/restore, resize, present, loss требуют живого окна |

Источники карты: [DrawOp](../../crates/flui-painting/src/display_list/command.rs),
[Layer](../../crates/flui-layer/src/layer/mod.rs),
[dispatch](../../crates/flui-engine/src/dispatch.rs),
[архитектура](../../crates/flui-engine/ARCHITECTURE.md).

## Исправления этого аудита

- Ключ кеша по CPU allocation теперь удерживает Image handle. Пока кеш или
  записанная команда используют адрес, allocator не может выдать его другому
  изображению. O(1) lookup и совместное использование клонами сохранены;
  пиксели не копируются. Цена: CPU-буфер живёт до удаления последнего ключа;
  GPU-бюджет кеша не является общим CPU+GPU лимитом.
- Content-derived ключи теперь включают width/height: одинаковый поток RGBA
  у 2×1 и 1×2 изображений означает разную геометрию текстуры.
- ImageFilter теперь сохраняет ordered DrawItems и заключительный сегмент:
  nested opacity не может удалять ранее записанный sibling или своё содержимое.
- Texture shader больше не отбрасывает alpha ниже 0.01: alpha 1/255 и 2/255
  влияют на фон, а прозрачный источник Clear всё равно очищает destination.
- Device feature selection ограничена advertised capabilities адаптера:
  native texture-format extension не запрашивается на неподдерживающем адаптере.

Во время аудита по просьбе пользователя подтянут origin/main до `84fc88220`.
Оттуда пришёл ADR-0099 и исправления destination-replacing layer region,
вложенного OffscreenTexture blend/clip и transparent Clear. Это исправления
upstream, не новые изменения аудита. Conflict resolution сохраняет их
clip-aware shader path и дополнительно убирает low-alpha cutoff. Дополнительный
readback проверяет прозрачный Clear как на корневом target, так и внутри
изолированной композиции с group opacity.

## Доказательства

- До обновления main: 52 engine lib tests passed, 0 skipped; после обновления:
  54 passed, 0 skipped. Запуски использовали `FLUI_REQUIRE_GPU=1` и serial GPU
  execution; отсутствие адаптера не считалось pass.
- Mutation run вернул пять дефектных решений: raw pointer без владения,
  content hash без размеров, alpha cutoff, отбрасывание ordered filter items,
  unconditional native feature request. Три выбранных теста упали именно на
  assertions, не при сборке: оба image-key cases, low-alpha case, все три
  Blur/Dilate/Compose rows, feature selection с empty capabilities. Production
  files автоматически восстановлены в finally. Это проверка чувствительности
  regression tests, не случайный красный запуск.
- Логи находятся в игнорируемом каталоге `target/engine-audit`: gpu-tests.log,
  gpu-main-tests.log, regression-mutations.log, gpu-final-tests.log. Финальный
  GPU-run после восстановления production-кода: 54 passed, 0 skipped,
  88.504 секунды. Scope gate записывает результат в check-changed.log.
- Визуально осмотрены PNG readback повёрнутого rect, Gaussian blur и вложенного
  Compose с красным, полупрозрачным синим и зелёным siblings: форма
  и halo соответствуют проверяемым сценам. Основное доказательство — pixel
  oracle, не визуальное впечатление. Отдельные nested filter PNG сохраняются
  по именам blur/dilate/compose; исторических golden-эталонов здесь не создано.
- Не проверены живое окно/presentation, другие ОС/GPU backends, device loss на
  настоящем драйвере, hardware HDR, throughput и расход энергии. CPU selector
  matrix доказывает subset request, но не browser WebGPU smoke.

## Оставшиеся дефекты и неполная поверхность

Ниже сценарии установлены по production-коду; если отдельный воспроизводящий
GPU-тест ещё не написан, это не результат выполненного readback. Они нужны
для следующей работы, а не утверждают полную проверку всех комбинаций.

| Источник | Различающий сценарий | Что исправлять |
|---|---|---|
| [images](../../crates/flui-engine/src/batches/images.rs) | Image с Clear поверх фона попадает в обычный SrcOver путь; 45°/90° CTM преобразует лишь две диагональные точки | Blend state в image runs, полный affine quad |
| [gradient shader dispatch](../../crates/flui-engine/src/batches/gradients.rs) | Bounds масштабируются, endpoints/radii остаются local; Repeat/Mirror/Decal пропускаются в match | Один coordinate contract и tile mode; gradient opacity проверить отдельным readback |
| [atlas dispatch](../../crates/flui-engine/src/painter/draw.rs), [image batching](../../crates/flui-engine/src/batches/images.rs) | Sprite Matrix4 сокращается до translation; обычная sprite ветка не применяет ambient CTM | Полный sprite transform и UV contract |
| [external replay](../../crates/flui-engine/src/replay/flush.rs) | Registered nearest sampler не читается: view передаётся в default linear sampler | Sampler descriptor в draw/replay, удалить неиспользуемые bind groups после wiring |
| [atlas](../../crates/flui-engine/src/atlas.rs) | Увеличенный opaque icon смешивается с transparent gutter; результат отличается от standalone ClampToEdge | Edge padding или clamped sampling без сжатия UV; тест после reset и рядом с другим icon |
| [state stack](../../crates/flui-engine/src/state_stack.rs) | Второй rounded clip заменяет первый SDF; elliptical rx/ry превращаются в один radius | Пересечение clip coverage, настоящие elliptical corners |
| [mask pipeline](../../crates/flui-engine/src/offscreen/mask.rs), [mask shaders](../../crates/flui-engine/src/shaders/masks/solid.wgsl) | Premultiplied полупрозрачный white child снова смешивается straight-alpha и темнеет; image shader mask подменяется white | Явная premul mask formula и реальные image masks |
| [vertices](../../crates/flui-engine/src/batches/paths.rs) | UV передаются, но textured shader binding отсутствует | Закончить textured vertices либо явно ограничить публичный контракт |
| [texture pool](../../crates/flui-engine/src/texture_pool.rs) | 16 idle 3840×2160 RGBA8 targets — около 506 MiB без live intermediates | Лимит bytes и peak/live accounting вместо одного count |
| [renderer diagnostics](../../crates/flui-engine/src/renderer.rs) | Uncaptured validation/OOM лишь логируется; результат кадра не обязан отражать GPU fault | Error state/containment и следующий успешный frame; не маскировать потерю damage debt |

Сводные основания архитектурного плана:
[Rust UI конкуренты](engine-rust-competitors.ru.md),
[Skia/Impeller, Compose/SwiftUI](engine-rasterizer-references.ru.md),
[требования платформ 2026+](ui-requirements-2026.ru.md).

## План улучшений

### Сначала корректность пикселей

1. **Один поток упорядоченных draw runs — реализован в первом foundation.** Убрать запрет sealing сегмента со
   stops. Хранить gradient stops в стабильных slices общего frame buffer либо
   отдельных буферах с bind groups на run. Тест: gradient → circle → rect и
   rect → gradient → image, разные stops у двух сегментов; последние draws
   закрывают предыдущие в точках пересечения. Проверить nested filter replay.
2. **Один scene renderer для окна и capture.** Выделить traversal/target/effects
   из Renderer; HeadlessRenderer пользуется тем же путём. Acquiring surface,
   presentation и readback остаются разными edges. Тестировать Follower,
   ShaderMask, BackdropFilter и их вложения на одинаковых targets. Если меняется
   межкрейтовый контракт — ADR; для внутреннего переноса достаточно mapping decision.
3. **Полный clip stack.** Выбрать coverage-mask или stencil+coverage для
   пересечения SDF/path/rotated rect и Difference, а не заменять ancestor новым
   SDF. Тест: два непересекающихся rounded corner cutouts, concave path,
   even-odd hole, difference и restore между siblings; применять к shapes,
   gradients, images, glyphs и offscreen composite.
4. **Аффинные image quads.** Instance хранит полное 2×2+translation или четыре
   угла; UV остаются привязанными к источнику. Единый путь для cached/external,
   repeat, atlas и nine-slice. Readback цветного checkerboard при 45°/90°,
   reflection, skew, nonuniform scale; точки вне quad обязаны оставаться фоном.
5. **Единая семантика blend/alpha.** Не терять фиксированные Porter–Duff у
   изображений и blend вложенного OffscreenTexture. Matrix/gamma фильтр должен
   использовать paint blend после преобразования пикселей. Сверять все modes
   с независимым CPU oracle для opaque и translucent source/destination,
   включая edge coverage. Straight/premultiplied источник — явный тип состояния.
6. **Эффекты и damage одного кадра.** Sigma_x/y не усреднять; учитывать CTM и
   выходной halo, поддержку non-blur backdrop определять явно. Advanced shape
   не должна менять retained pixels вне damage даже на один кадр: пересечение
   при composite или promotion до записи. Сравнивать каждый partial frame с
   полной перерисовкой, включая moving blur, nested masks и изменённый фон.

### Затем надёжность и наблюдаемость

7. **Валидация до wgpu.** Проверять zero/over-limit размеры, buffer lengths,
   overflow, external texture device/format/usage и разумный total budget.
   Невалидный input возвращает EngineError или явную диагностируемую
   неподдерживаемую операцию; после отказа следующий frame успешно рисуется.
8. **Ограниченные ресурсы.** Измерять общий CPU/GPU footprint: cached Image
   handles, atlas, glyph font registry, offscreen pool, pipelines и buffers.
   Viewport-size nested filters корректны, но дороги; bounds/rebase вводить
   только после readbacks. Stress: тысячи смен картинок/шрифтов/размеров и
   контроль plateau, eviction и повторного upload после reclaim.
9. **Живое окно и восстановление.** Windows DX12 smoke с resize, minimize,
   restore, release/recreate, DPR changes и pre-present hook. Отдельно проверить
   adapter compatible_surface, реальный surface colour space и восстановление
   устройства; fake handles не подтверждают работу драйвера.
10. **Переносимая визуальная матрица.** После совпадения capture с production
    добавить contact sheet demo-сцен: typography, images, nested clipping,
    filters, blend, gradients, fractional DPR. Golden хранит backend/adapter,
    format, DPR, fonts и осмысленный tolerance; выдаёт actual/expected/diff.
    Аналитические pixel assertions остаются основным oracle для blend и AA.
    Проверять DX12/WARP, Vulkan/software, Metal и WebGPU по доступности хостов.
11. **Производительность после корректности.** Замерять cold/warm frames,
    p50/p95/p99 CPU record/encode и GPU timing, uploads, passes, draw calls,
    offscreen bytes и retained blit. Использовать существующие benches и
    сравнивать с baseline одного адаптера; не добавлять culling/batching,
    меняющий порядок, без readback, различающего ошибочный результат.

### Решения, которые нельзя скрывать в оптимизации

- Цвет сейчас encoded SDR UNorm/Srgb presentation; HDR, wide gamut и linear
  blending требуют end-to-end color-management ADR, не смены texture format.
- Content snapping по ADR-0098 требует признака animated layer и независимых
  readbacks для static/animated; округление каждого quad вслепую ломает анимацию.
- Threaded raster lane следует сначала связать с production realm или удалить
  неиспользуемую поверхность. Параллелизм не должен приносить node locks в frame path.
- Поддержка perspective, shader-on-arbitrary-path, больших stop tables и
  нестандартных external texture formats должна стать явным контрактом;
  принимать параметр и незаметно заменять результат приближением недостаточно.

## Как повторить проверку

Один compiling worker: установить `CARGO_BUILD_JOBS=1`. Для GPU-run установить
`FLUI_REQUIRE_GPU=1`, чтобы отсутствие адаптера не стало ложным pass;
`FLUI_READBACK_DUMP_DIR` — абсолютный путь для PNG.

```text
cargo xtask --help
cargo nextest run --locked -p flui-engine --features testing --lib --no-fail-fast --test-threads 1
cargo xtask gpu-test
cargo xtask check-changed
```

Feature `testing` включает дополнительные GPU suites, обычный check-changed
не заменяет этот запуск. `cargo nextest` не запускает doctests.

## Историческая проверка перед foundation implementation

cargo xtask checks прошёл повторно (target/engine-audit/checks-plan-review.log).
cargo xtask check-changed прошёл fmt, workspace clippy, 607/607 nextest (10 skipped),
strict docs/doctests и platform typechecks; затем flui-app Android C dependency
остановилась: clang отсутствует в PATH. Полный gate не green; CI/cross-app Android
совместимость этим запуском не доказана. Лог: target/engine-audit/check-changed-final.log.
На момент этого запуска foundation оставался планом; повторное adversarial review описано в
[решениях](../plans/engine-plan-rereview.ru.md).

## Реализация первого foundation

Ordered/sealed IR, immutable stop/viewport параметры, bounded recording arenas,
DeviceDomain admission/retirement и candidate/committed target reuse внедрены.
Финальный GPU run: 58/58, 0 skipped, `FLUI_REQUIRE_GPU=1`, включая восстановление
после отказа и квоту на две retained texture. Журнал:
`target/engine-audit/foundation-gpu-final.log`. Mutation run действительно теряет
нужные pixels при возврате трёх исправленных дефектов.

[Измерения и ограничения](engine-foundation-measurements.ru.md) фиксируют цену
правильного порядка, оптимизацию replay и candidate reuse. Общего обещания
ускорения нет. [План](../plans/engine-foundation-implementation.ru.md) отделяет
этот результат от ещё не реализованных resource/color/producer/diagnostic контрактов.

Финальный `cargo xtask check-changed` после реализации завершился с exit 0:
608/608 nextest, 10 skipped; workspace и testing clippy, strict docs/doctests,
platform typechecks, Android app, desktop-mcp Windows/macOS и wasm checks прошли.
На Windows не запускались Linux/xvfb platform suite и iOS app runner; это остаётся
за CI. Android C dependency проверена с установленными LLVM clang/llvm-ar через
локальное окружение команды, без изменения gate. Журнал:
`target/engine-audit/foundation-check-changed.log`.
