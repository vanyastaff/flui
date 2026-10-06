# render-proof — дизайн (уровень 1)

- **Статус:** черновик (ревью: approve with fixes, правки внесены)
- **Дата:** 2026-10-05
- **База:** `main` @ `4915054c8`
- **Требования:** [requirements.md](requirements.md) (R1–R16, открытые вопросы 1–3)

Документ выбирает место readback API, согласование устройства, эталоны, негативные контроли
и снимок окна; отвечает на вопросы 1–3 и задаёт ADR, заменяющее ADR-0087 §2.

## Текущее состояние

- **Два пути записи кадра.** Публичный `HeadlessRenderer::render_layer_tree` обходит дерево
  напрямую через `layer_walk::record_layer_tree` (`crates/flui-engine/src/headless.rs:224-227`),
  без `FrameProtocol` и `select_font_source`. Окно пишет кадр через
  `Renderer::record_frame_content` (`crates/flui-engine/src/renderer.rs:1921-1946`: сброс
  состояния, scissor, trace `Damage scissor applied`) внутри `FrameProtocol::plan/run`.
  Тот же путь вместе с `FontSource::Ordinary` повторяет `RetainedCapture`
  (`headless.rs:448-475`, `:697-745`), но он `#[cfg(test)]`, `pub(crate)`, а его комментарий
  (`headless.rs:438-440`) откладывает публичный golden API до CPU-бэкенда ADR-0087 §2.
- **Backend и адаптер.** На Windows engine компилирует только DX12
  (`crates/flui-engine/Cargo.toml:104-105`), на Linux — только Vulkan (`:110-111`). Headless
  создаёт `Instance` на всех backend'ах (`headless.rs:104`), окно — на `select_backend()`
  (`renderer.rs:1082`, `:831-838`). Оба запрашивают адаптер через `trusted_adapter_options`
  с `HighPerformance` и `force_fallback_adapter: false` (`adapter.rs:29-39`). Окно передаёт
  `compatible_surface`, headless — `None` (`headless.rs:104-111`), так что на гибридной
  системе (iGPU + dGPU) они могут выбрать разные адаптеры. Поэтому на GPU-хосте WARP сейчас
  недостижим. `WGPU_ADAPTER_NAME` читает только `wgpu::util::initialize_adapter_from_env`
  (wgpu 30.0.1, `src/util/init.rs:16`); `request_adapter` engine его не видит.
- **Формат и путь.** Capture рендерит в `Rgba8Unorm` (`headless.rs:26`). Окно выбирает
  `Bgra8Unorm` или `Rgba8Unorm` с `SurfaceColorSpace::Srgb` (`renderer.rs:1260-1283`). Ошибка
  в выборе (например, `Bgra8UnormSrgb` с повторным кодированием) headless не видна. Окно
  рендерит напрямую в swapchain, только если поверхность даёт `COPY_SRC`; иначе каждый кадр
  идёт через retained target и blit (`renderer.rs:674-682`). Текстура capture всегда имеет
  `COPY_SRC`, так что headless проверяет только прямой путь. Фон — общая
  `frame_protocol::background_clear_value()` (`crates/flui-engine/src/frame_protocol.rs:27`).
- **Skip/fail** (`crates/flui-engine/src/test_support.rs:300-332`) только `#[cfg(test)]`
  (`lib.rs:396-397`). Паника растра окна фатальна (`raster_owner.rs:364`), не `EngineError`.
- **Reach.** Тир K запрещает `flui-engine` и `wgpu` (`Cargo.toml:132-143`); `flui-testing` —
  тир K (`crates/flui-testing/Cargo.toml:117-124`). Facade — тир H (`Cargo.toml:634-638`) и
  уже получает engine через `flui-app` (`crates/flui-app/Cargo.toml:85`).
- **Harness.** `widgets::lay_out` принимает только `BoxConstraints`
  (`crates/flui-testing/src/widgets.rs:307`); поверхность — `ceil` логического (`:275-290`);
  DPR = 1.0 (`crates/flui-testing/src/realm.rs:146-148`). Sink хранит последний `Scene`
  (`realm.rs:237-241`), наружу отдаётся только `&LayerTree` (`widgets.rs:1262`).
  `HeadlessBinding::mount_root` строит `RootRenderView` из логического размера без DPR
  (`crates/flui-testing/src/bootstrap.rs:321-348`).
- **LayerTree** только растёт: `new`, `push_child`, `get` (`crates/flui-layer/src/tree/layer_tree.rs:134-201`).
  `Clone` у дерева нет, у `Layer` есть (`crates/flui-layer/src/layer/mod.rs:80`).
- **Окно на Windows.** Stdout пробы уходит в null без `FLUI_PROBE_RUST_LOG`
  (`tools/xtask/src/device/uia.rs:86-101`). `PrePresentHook` — `FnMut()` без доступа к
  текстуре (`crates/flui-engine/src/raster.rs:23`). Маркер `event="surface_released"` —
  `crates/flui-engine/src/surface_lease.rs:205`. Проверка порядка есть только в
  `tools/live-smoke/src/self_close.rs:81-140` (якорь winit). У Win32 нет trace на
  `WM_DESTROY`. `recover()` проверяет владельца до работы с GPU (`renderer.rs:1024-1035`).
  `xcap` с фичей `wgc` проверен в `tools/desktop-mcp/Cargo.toml:56-58`.

## Варианты

### (a) Где живёт readback API — вопрос 1

| Вариант | Reach и тир | Цена | Риск |
|---|---|---|---|
| A1. `flui-testing` с зависимостью на engine | K → engine/wgpu: `reach` отказывает, нужен `reach-exceptions` с ADR | мало кода | ломает правило «K не видит GPU» |
| A2. Только engine + `LaidOut::scene()` | без изменений | каждый пользователь пишет свой glue (размер, DPR, damage, skip) | R10 «одной функцией» не выполняется |
| **A3. Сравнение без GPU — в `flui_testing::pixels`, захват — в `flui::testing::gpu` за `testing-gpu`** | K не трогает GPU; новый crate в графе не появляется | две точки | минимальный |

**Выбор: A3.** Ответ на вопрос 1: API R1 — `flui::testing::gpu` за фичей
`testing-gpu = ["testing", "dep:flui-engine", "flui-engine/testing", "dep:pollster"]`.
Элементы захвата в engine стоят за его существующей фичей `testing` («не часть стабильного
API», `crates/flui-engine/Cargo.toml:39-44`), а не в стабильной поверхности. `pollster`
нужен только в test-only коде: фича `testing-gpu` подключается только в dev-dependencies, и
обычный граф `flui` его не получает (release R4). Правило skip/fail —
`flui_engine::headless::gpu_or_skip`: его зовут и наборы engine, и facade
(`skip_unless_gpu`).

### (b) Согласование устройства — вопрос 2 и обязательство R1

B1 (устройство wgpu по умолчанию, различие задокументировано) проверяет пиксели при чужих
limits/features и, возможно, чужом backend'е. **Выбор: B2** (`select_backend()` +
`request_flui_device` + общий путь записи + тестовая подмена адаптера, ~2 дня). Ответ на вопрос 2: да, через `request_flui_device`. `HeadlessRenderer::acquire`
строит `Instance` на `Renderer::select_backend()`, вызывает `GpuCapabilities::detect` и
`request_flui_device`. Двойник `without_dual_source_blending` подменяет `GpuCapabilities`.
Абзацы про «default device» в `adapter.rs:11-13` и `headless.rs:120-124` удаляются.

**Подмена адаптера (только `testing`).** Под фичей `testing` `acquire` читает
`FLUI_TEST_ADAPTER=default|fallback`. `fallback` ставит `force_fallback_adapter: true`: на
DX12 это WARP, на Vulkan — программный ICD хоста (lavapipe в WSL2). Без переменной поведение
равно `default`. Выбранное значение пишется в `AdapterSummary` и в отчёт R16. Оконный путь
переменную не читает. `WGPU_ADAPTER_NAME` не используется: engine не вызывает
`initialize_adapter_from_env`. Lavapipe замеряется только в WSL2: на Windows engine Vulkan не
компилирует, так что `mesa-dist-win` недостижим.

**Тот же обход, что у окна (R1).** Публичный захват — `FrameCapture`, повышенный из
`RetainedCapture`. Кадр проходит `FrameProtocol::plan` → `FrameProtocol::run` →
`Renderer::record_frame_content` → `layer_walk::record_layer_tree` с
`select_font_source(FontSource::Ordinary)` — те же функции, что у `Renderer::render_scene`.
Свои у захвата только шаги `FrameSteps`: очистка той же `background_clear_value()`,
цель-текстура вместо swapchain и blit. `render_layer_tree` держит кэш `FrameCapture` по
размеру: painter, атлас и конвейеры живут между вызовами. Второй обход удаляется,
`examples/screenshot.rs` не меняется.

**Известные расхождения с окном.** Перечислены явно, а не отрицаются:

| Расхождение | Чем закрыто |
|---|---|
| формат поверхности: capture фиксирован на `Rgba8Unorm`, окно — `select_surface_format` | R15: точки средних тонов (повторное кодирование 0x80 даёт ≈0xBC; 0 и 255 sRGB не меняет) |
| прямой путь против retained target + blit на поверхности без `COPY_SRC` | engine-строка калибровки с `require_intermediate` в обе стороны; `windows-notes` пишет `copy_src` поверхности в trace и отчёт |
| выбор адаптера (`compatible_surface` против `None`) | R16 пишет оба адаптера: окна (из trace `Selected GPU`) и захвата |
| композиция DWM, порядок BGRA, DPR от ОС, resize посреди кадра | R15 (только точки), R12 |

Вариант **in-window pre-present readback** (хук с доступом к swapchain или retained-текстуре,
отдающий точные байты окна в реальном формате; WGC — только для DWM) рассмотрен и
**отклонён на этот релиз**. Он требует новой production-поверхности в `Renderer`
(`PrePresentHook` сейчас `FnMut()`), `COPY_SRC` на swapchain (которого может и не быть —
ровно случай retained), синхронного ожидания GPU в кадре и канала вывода байтов из процесса
Notes (env-переключатель в поставляемом runner'е или в примере). Это около 3 дней и
постоянная поверхность. Ловит он то же, что WGC со средними тонами и engine-строка
`require_intermediate`, кроме точных байтов на AA-краях, которые R15 и так не сравнивает.
Это follow-up, если R15 найдёт расхождение, которое WGC не объясняет.

### (c) Эталоны и сравнение — вопрос 3

C1 (только точки) не видит удалённый слой и пропущенный clip вне точек. C3 (перцептивное
сравнение, как `egui_kittest`: порог 0.6, `failed_pixel_count_threshold` 0) не связано с ±2
R3. **Выбор: C2** — точки, затем кадр по правилу R4; цена — эталоны PNG в git.

- **Порядок.** Размер → точки R3, краевые точки R8 → правило R4. Считаются все проверки,
  возвращается множество проваленных; отказ начинается с R3 и называет число отличий,
  первые 8 координат и путь дампа (`FLUI_READBACK_DUMP_DIR`).
- **Правило R4** — константы в тесте: `channel_tolerance`, `max_differing_pixels`. Маска
  glyph runs (прямоугольники текстовых команд закоммиченного `Scene` в device px, +1 px) и
  её pub-тип появляются, только если замер выберет (б). Полосы вдоль краёв нет.
- **Точки.** Геометрия — `LaidOut::absolute_offset`/`size` × DPR, внутрь на 2 device px.
  Цвет — константы `ThemeData::light()`. Глиф-точки — на стволе глифа заголовка. У каждого
  экрана есть хотя бы одна точка среднего тона.
- **Хранение.** `tests/references/notes/<state>@<dpr>[-rtl].png` (RGBA8, крейт `image`):
  ~24 файла, ~3 МБ по оценке. Рядом лежит sidecar `<state>@<dpr>[-rtl].adapter.txt`
  (`AdapterSummary`, `driver_info`, сборка ОС, значение `FLUI_TEST_ADAPTER`) и
  `samples.txt` (точки и цвета для DPR 1.0 и 1.5; GPU-тест сверяет его с константами темы,
  `windows-notes` читает).
- **Класс адаптера (только для (в))** — типизированный ключ
  `(backend, device_type, vendor, device)`, не имя (AGENTS «Identity is not a label»).
  Каталог — `tests/references/notes/<class>/`.
- **Вопрос 3.** При (в) эталонный класс — **WARP** (`FLUI_TEST_ADAPTER=fallback` на
  Windows). Он воспроизводим на любом Windows (D1) и не зависит от драйвера мейнтейнера.
  Класс для железа появляется, только если оно не проходит (б) против WARP.
- **Несовпадение адаптера.** При (в) запуск на другом классе падает с текстом
  «recorded on X, running Y» без попиксельного diff. При (а)/(б) отказ R4 начинается с той же
  строки, если драйверы различаются.

**Замер — первая задача и go/no-go.** Тот же бинарь с `FLUI_READBACK_DUMP_DIR` снимает строки
R2 и все контроли R5 на WARP (`fallback`), железе (`default`) и lavapipe (WSL2, `fallback`).
Для каждой пары адаптеров и каждого контроля против эталона считается гистограмма
`max |Δ|` и число пикселей с отличием больше t, с маской и без. Выбирается первая структура
из (а), (б), (в), при которой для некоторого t межадаптерный максимум N₀ таков, что каждый
контроль даёт больше 2·N₀ отличий выше t. Числа и таблица идут в `docs/testing.md`
«Determinism». Ожидание по прежнему замеру (`docs/testing.md:827-851`) и ADR-0067 (глифы на
CPU): (а) или (б). **No-go:** если и (в) не разделяет контроли на одном классе (AA-края
нестабильны даже внутри WARP), план меняет форму. R4 сводится к сравнению только внутри
класса, краевые точки R6/R8 — к engine-калибровке, а snapping ADR-0098 §6 выносится владельцу
отдельным вопросом до п. 6.

### (d) Внедрение поломки для R5

D1 — `#[cfg(test)]`-переключатели painter: применимы в engine in-src, калибровочной сцене
Notes не нужен; из facade недоступны, а cargo-фича в published crate отвергнута (ADR-0087
«Alternatives»). D2 — пересборка закоммиченного `Scene` (`LayerTree::new/push_child`,
`Layer: Clone`): для дерева Notes, но только для поломок, выразимых слоями. **Выбор: оба по
месту** — калибровка и цветовые конвенции D1 в engine, структурные контроли D2 в facade.

Engine in-src, `calibration_detects_broken_conventions` (существующий табличный раннер
engine, nextest-группа `gpu-readback`):

| Строка | Сцена / переключатель | Ожидание |
|---|---|---|
| `calibration` | 50 % альфа над известным цветом, двухстоповый градиент, скруглённый clip | аналитические значения ±2, краевые точки на дробных позициях — смешанное значение |
| `calibration_through_intermediate` | то же с `require_intermediate` | тот же результат, что прямой путь |
| `glyph_edge` | глиф на целом origin | ожидаемое частичное покрытие берётся из детерминированного CPU-растеризатора (`SwashRasterizer`, ADR-0067) |
| `straight_alpha_blend_is_rejected` | переключатель blend state конвейера: premultiplied ↔ straight | проваливает альфа-точку |
| `linear_blending_is_rejected` | цель захвата как `Rgba8UnormSrgb`-view: смешивание и запись в linear (это и баг повторного кодирования) | проваливает точку градиента и альфа-точку |

Facade, `notes_reference_rejects_broken_renders` (D2):

| Контроль | Мутация | Должен провалить |
|---|---|---|
| удалён слой | поддерево разделителя вне точек пропущено | `Frame` |
| сдвиг 1 логический px | `Offset(1, 0)` над заголовком | `Samples` (глиф-точка) |
| сдвиг ½ device px | `Transform` на 0.5/DPR над app bar | `Frame` + краевая `Samples` |
| clip +1 device px | `ClipRect` поля с ошибкой расширен на 1/DPR | краевая `Samples` |
| straight ↔ premultiplied | полупрозрачное поддерево с постоянной a в `ColorFilter` rgb×(1/a); точка стоит на опакном фоне | `Samples` (точка над опакным белым: c + (1−a)·d против c·a + (1−a)·d) |
| неверная альфа opacity | `Opacity(0.9)` над app bar | `Samples` |
| пропущен clip | `ClipRect` заменён детьми | `Frame` |
| DPR ×2 (1.0 и 1.5) | корневой `Transform` ×2, размер readback тот же | `Samples` |
| зеркальная раскладка | RTL-фикстура против точек и эталона LTR | `Samples` |

**Чего контроли на уровне слоёв не выражают** (их ловят только engine-наборы и их
собственные тесты): премультипликация, разная у разных конвейеров; покрытие, умноженное
только на rgb без альфы; запасной путь без dual-source blending (feathered fringe,
`without_dual_source_blending`); полутексельный сдвиг атласа и бины субпиксельного
позиционирования глифов.

### (e) Снимок окна на Windows в xtask (R15)

| Вариант | За | Против |
|---|---|---|
| **E1. `xcap` с фичей `wgc`** | одно окно, включая DirectComposition (`DxgiFromVisual`, `renderer.rs:820-830`); проверен в `desktop-mcp` | кадр с рамкой: нужен crop; жёлтая рамка Windows 10 лежит вне client area |
| E2. DXGI Desktop Duplication | точный вывод монитора | весь output, окно не должно быть перекрыто, много `unsafe` |
| E3. `PrintWindow(PW_RENDERFULLCONTENT)` | один вызов | флаг не документирован; DComp без него даёт чёрный кадр (Chromium/WebRTC) |

**Выбор: E1**, `tools/xtask` под `cfg(windows)`, версия и фича — как в `desktop-mcp`.
Окно ищется по HWND. Свёрнутое окно (`IsIconic`) не снимается — `CANNOT_VERIFY`. Crop:
origin = `ClientToScreen(0,0)` − origin `DWMWA_EXTENDED_FRAME_BOUNDS`, размер = client rect.
Проверяется, что размер равен логическому client × DPI/96; иначе `CANNOT_VERIFY`. Точки
берутся из `samples.txt` (средние тона включены), геометрия — из UIA bounds того же элемента.
Масштаб 100 % и 150 % выставляет мейнтейнер; драйвер сверяет `GetDpiForWindow` с `--scale`.
Это **датированный ручной gate**, а не автоматический: CI его не исполняет.

## Публичный контракт

Каждый элемент вызывается из публичного API тестирования facade, кроме engine-элементов за
`testing`. В стабильной поверхности engine ничего не прибавляется.

```rust
// flui-engine, только под существующей фичей `testing` (не стабильный API)
#[cfg(feature = "testing")] pub mod headless {
    pub struct FrameCapture { /* FrameProtocol, painter, retained target, size */ }
    impl HeadlessRenderer {
        pub fn adapter_info(&self) -> &wgpu::AdapterInfo;
        /// Кадры через `FrameProtocol`, как у окна; первый кадр всегда полный.
        pub fn frame_capture(&self, size: (u32, u32)) -> EngineResult<FrameCapture>;
    }
    impl FrameCapture {
        /// `Some(rect)` — кадр отрисован частично с этим scissor; `None` — полностью или без изменений.
        pub fn render(&mut self, scene: &flui_layer::Scene, damage: flui_layer::DamageRegion)
            -> EngineResult<Option<flui_layer::DamageRect>>;
        pub fn read_rgba(&self) -> EngineResult<Vec<u8>>;
    }
    /// `FLUI_REQUIRE_GPU` задан → panic с причиной; иначе `skipping: no usable GPU (<reason>)` и `None`.
    pub fn gpu_or_skip<T, E: std::fmt::Display>(acquired: Result<T, E>) -> Option<T>;
}
// Новых вариантов EngineError нет: оконный путь паникой EngineError не производит.

// flui-testing (тир K), реэкспорт под flui/testing
pub struct MountOptions { /* поля приватные */ }
impl MountOptions {                      // new/tight остаются; добавляется:
    pub fn physical(size: DeviceSize, ratio: DevicePixelRatio) -> Self;
}
pub mod widgets {
    pub fn lay_out_with(root: impl View, options: MountOptions) -> LaidOut;
    impl LaidOut {
        pub fn scene(&self) -> Option<&flui_rendering::layer::Scene>;
        pub fn surface_size(&self) -> DeviceSize;
    }
}
pub mod pixels {
    pub struct Readback;   // new(size, rgba) -> Result<_, PixelError>; size(), pixel(DevicePoint), as_rgba()
    pub struct SamplePoint { pub name: &'static str, pub at: DevicePoint, pub expected: [u8; 4] }
    #[non_exhaustive] pub struct FrameRule { pub channel_tolerance: u8, pub max_differing_pixels: u32 }
    #[non_exhaustive] pub enum Check { Size, Samples, Frame }
    pub struct Comparison; // failed() -> &[Check], compared_pixels(), Display с первыми отличиями
    pub fn compare(actual: &Readback, reference: &Reference, samples: &[SamplePoint],
                   rule: &FrameRule) -> Comparison;
    pub enum ReferenceMode { Verify, Update }   // from_env(row): FLUI_UPDATE_REFERENCES=<glob>
    pub struct Reference;  // open(path, mode) -> Result<Reference, ReferenceError>; sidecar рядом
    #[non_exhaustive] pub enum ReferenceError { Missing, Unreadable, Refused(Comparison), Updated { path } }
}

// flui (facade, тир H), фича testing-gpu; типов wgpu нет
pub mod testing::gpu {
    pub struct GpuCapture;   // HeadlessRenderer + кэш FrameCapture по размеру + LayerDiffer
    impl GpuCapture {
        /// Единственный конструктор: позже он сможет принять GpuContext (ADR-0091 §4).
        pub fn new() -> Result<Self, CaptureError>;
        pub fn adapter(&self) -> &AdapterSummary;
        /// Последний закоммиченный Scene; damage от production `LayerDiffer`, как в raster lane.
        pub fn capture(&mut self, laid: &LaidOut) -> Result<Captured, CaptureError>;
        pub fn capture_scene(&mut self, scene: &Scene, size: DeviceSize) -> Result<Captured, CaptureError>;
    }
    pub struct Captured { pub pixels: Readback, pub damage: Option<DeviceRect> }
    #[non_exhaustive] pub struct AdapterSummary { pub name: String, pub backend: String,
        pub driver: String, pub driver_info: String, pub device_type: String,
        pub vendor: u32, pub device: u32, pub test_adapter: String }
    #[non_exhaustive] pub enum CaptureError {
        GpuUnavailable(Box<dyn std::error::Error + Send + Sync>),
        NothingCommitted,
        Render(Box<dyn std::error::Error + Send + Sync>), // и пойманная паника записи
    }
    pub fn skip_unless_gpu<T>(acquired: Result<T, CaptureError>) -> Option<T>; // = gpu_or_skip
}
```

**`MountOptions`.** Внутри лежит `enum RootSurface { Constraints(BoxConstraints),
Physical { size: DeviceSize, ratio: DevicePixelRatio } }`; ограничения выводятся из варианта:
логическое = физическое / DPR (`ViewConfiguration::from_size`). Публичные поля `constraints`
и `capabilities` становятся приватными, вызовы мигрируют на конструкторы и
`with_capabilities` (часть п. 3). `HeadlessBinding::mount_root` DPR **соблюдает**: логический
размер корня выводится из варианта, DPR передаётся в конфигурацию `RootRenderView`, корневой
`TransformLayer` его несёт. Захвата у `Mounted` нет: readback идёт только через `LaidOut`.

Фичи: `gpu-readback-tests = ["testing-gpu", "material"]` остаётся gate'ом GPU-бинаря.
`TEST_FEATURES` (`tools/xtask/src/tasks.rs:71`) получает `flui/testing-gpu`, чтобы GPU-free
строки шли в CI. Изменение API фиксируется в снимке поверхности facade (release R3) и в
`changelog.d/`; новая фича проходит `cargo xtask facade-combos`.

**Без публичной поверхности:** мутации контролей, переключатели painter (`#[cfg(test)]`),
отчёт R16 (тестовый бинарь и xtask), `windows-notes`, строки `surface_lifecycle_matrix`,
in-src seam паники в `GpuCapture`. Единственная production-правка вне engine и harness —
trace `tracing::debug!(target: "flui.platform", event = "native_window_destroyed")` в ветке
`WM_DESTROY`.

## Инварианты

- **Один путь записи, одно семейство backend'ов.** Пиксели `flui::testing::gpu` получены
  через `FrameProtocol` и `record_frame_content` на устройстве из `request_flui_device`, на
  семействе backend'ов из `select_backend()`. Адаптер может отличаться (`compatible_surface`,
  `FLUI_TEST_ADAPTER`), и тогда отчёт это показывает. Второго обхода дерева нет.
- **Шрифты.** `FontSource::Ordinary`; тестовый realm строит `FontCollection::new()` только со
  встроенными гарнитурами (`docs/testing.md:856-866`).
- **DPR и размер.** Физический размер целый, логический = физический / DPR, readback имеет
  физический размер. DPR задаётся только через `DevicePixelRatio::new` (ADR-0098).
- **Фон** опакный, из `background_clear_value()`.
- **sRGB.** Цель UNorm без `*Srgb`-представления, смешивание в закодированном sRGB.
  Повторное кодирование в окне ловят точки средних тонов R15. Захват окна — только при
  выключенных HDR/Auto Color Management, иначе `CANNOT_VERIFY`.
- **Время.** Кадр снимается после `settle` на виртуальных часах (переход маршрута 300 мс);
  каретка и индикатор зависят только от виртуального времени; указатель не наводится.
- **Отказы.** Нет адаптера или устройства → `GpuUnavailable`, никогда не `Ok`. Device lost →
  `Render`. Паника при записи ловится в `GpuCapture` (`catch_unwind`); payload уничтожается
  через существующий helper удержания (ADR-0127), кэш `FrameCapture` и `LayerDiffer`
  выбрасываются, следующий захват полный. Частичный кадр как успех не возвращается.
- **Эталон.** `Update` сначала прогоняет Size, Samples и калибровку и отказывается писать
  (`Refused`), если любая провалилась. После записи тест всё равно падает (`Updated`).
  `FLUI_UPDATE_REFERENCES=<glob>` выбирает строки; CI его не выставляет.

## Тестовая стратегия

GPU-строки facade идут в `tests/gpu_readback/main.rs` (`[[test]] gpu_readback`,
`required-features = ["gpu-readback-tests"]`), туда модулем переносится
`composited_layer_update_readback`. Обновляются группа `gpu-readback` в
`.config/nextest.toml:89` (новый бинарь, плюс `capture_failure_matrix` и
`calibration_detects_broken_conventions` в engine) и `gpu_test_plan` (`tasks.rs:530-553`).

| R | Тест | Тир / где | Без изменения падает потому, что |
|---|---|---|---|
| R1 | `notes_home_readback_has_the_committed_size` (DPR 1.0 и 1.5) | GPU, `gpu_readback` | API нет; при `ceil` логического размер на 1.5 не совпадёт |
| R2–R4 | `notes_screens_match_their_references`: 6 состояний × DPR {1.0, 1.5, 2.0} + `*_rtl` | GPU, `gpu_readback` | нет эталона и сравнения; сломанный кадр проваливает `Frame` (см. R5) |
| R5 | `notes_reference_rejects_broken_renders` (D2) | GPU, `gpu_readback` | строка требует, чтобы названная проверка провалилась, а `Size` прошла |
| R5, R6 | `calibration_detects_broken_conventions` (D1) | GPU, engine in-src (приватные переключатели) | без калибровки и переключателей конвенции не проверяются |
| R7 | `notes_partial_frames_match_full_frames`: захват A до/после, свежий B после | GPU, `gpu_readback` | предусловие `damage.is_some()`; вне damage побайтно, внутри ±0 (±2 только с причиной). Engine-тест `partial_equals_full_inside_damage` дерево Notes не видит |
| R8 | `mount_at_dpr_derives_logical_size` (1.0/1.5/2.0, нечётная ширина; `lay_out_with` и `mount_root`) | Widget, `flui-testing` `tests/main.rs` | конструктора нет; при `ceil` логическое ≠ физическое/DPR |
| R9 | строки `*_rtl` | GPU | без RTL ведущая иконка у левого края |
| R10 | `notes_readback_without_adapter_is_an_error`: дочерний процесс того же бинаря с `WGPU_BACKEND`, не называющим собранный backend; ветки с `FLUI_REQUIRE_GPU` и без | Widget, in-src `src/testing/gpu.rs`; отдельная запись nextest-группы для дочернего процесса | `Ok` вместо `GpuUnavailable` или skip при заданной переменной |
| R11 | `capture_failure_matrix`: device lost (`device.destroy()`), паника в content, обе подряд, следующий захват на новом renderer | GPU, engine in-src (`run_table`) | второй вызов отдаёт пиксели или кэш переживает unwind |
| R11 | `a_panicking_capture_is_an_error_and_the_next_is_full` | GPU, facade in-src (приватный seam паники в `GpuCapture`) | без `catch_unwind` паника выходит из `capture`; без сброса кэша следующий кадр частичный |
| R12 | `an_outdated_surface_drops_the_frame_without_presenting`, `a_resize_mid_frame_renders_the_next_frame_at_the_new_size` в `surface_lifecycle_matrix` | Unit, `flui-app` in-src | fake фиксирует present старого размера |
| R12 | шаг «нет растяжения после resize» в `windows-notes` | native | растянутый кадр сдвигает край app bar |
| R13 | (а) порядок `surface_released` < `native_window_destroyed` в логе Alt+F4; (а) `close()`, (б), (в) — `closed_window_surface_matrix`, модулем в `crates/flui-app/tests/main.rs` (`cfg(windows)`, `#[ignore]`, запуск из `device windows-notes`) | native | без lease поверхность освобождается после destroy; без probe `recover()` даёт `SurfaceCreation`; без обработки `Lost` callback-close падает |
| R14 | `reference_update_requires_explicit_opt_in`: `Verify` на отсутствующем файле → `Missing`, файл не создан; `Update` при провале Samples → `Refused`, файл не создан; `Update` при успехе → файл и sidecar есть, `Updated`; glob фильтрует строки | Widget, `flui-testing` | без проверки режима файл пишется молча |
| R15 | шаг `windows-notes`: xcap при 100 % и 150 %, точки (включая средние тона), `Damage scissor applied` после ввода, `copy_src` поверхности | native, ручной gate | без damage строки нет; повторное кодирование сдвигает средний тон |
| R16 | `render_proof_report_names_every_field` на фиктивных строках | Widget, xtask in-src | пропущенное поле (оба адаптера, `FLUI_TEST_ADAPTER`, пиксели, контроль, масштаб, `copy_src`) не замечается |

**Различение (AGENTS «Regression tests that pass both ways»).** Для R1, R8, R10, R11 и R14
production-правка откатывается в изолированном worktree, прогон должен упасть по нужной
причине; запуски перечисляются в PR. Для R2–R6 сломанный рендер подставляют сами контроли.

## ADR

**ADR (номер назначит оркестратор): Golden-image proof is a wgpu readback through the
windowed frame path.** `Supersedes: ADR-0087 §2`; в ADR-0087 добавляется `Superseded-by` для
§2, остальные разделы не меняются.

> Решение. Golden-доказательство пикселей FLUI — readback wgpu, а не CPU-бэкенд;
> `flui-engine-cpu` не создаётся, пиксельный CI без GPU не цель. Доказательство
> выполняется локально (`cargo xtask gpu-test`) и в датированном нативном прогоне Windows.
> Headless-захват проходит `FrameProtocol` и `Renderer::record_frame_content` на устройстве
> из `request_flui_device` и семействе backend'ов из `select_backend()`. Известные
> расхождения с окном (формат поверхности, прямой путь против retained, выбор адаптера,
> композиция DWM) перечислены и закрыты точками средних тонов в окне, строкой калибровки
> через retained target и записью обоих адаптеров в отчёт. Публичный захват —
> `flui::testing::gpu` (тир H); сравнение без GPU — `flui_testing::pixels`, тир K GPU не
> видит. Правило кадра задаётся замером на WARP, lavapipe и железе и подтверждается
> негативными контролями; эталонный класс, если нужен, — WARP.

Следует: ADR-0063 (R13), ADR-0100 (тот же candidate/commit), ADR-0098 (типизированный DPR;
§6 вне scope, при его приходе эталоны пересъёмываются), ADR-0081 §2 (reach не меняется),
ADR-0091 §4 (`GpuCapture::new` — единственный конструктор). ADR-0101 не затронут.
Обновляются `crates/flui-engine/ARCHITECTURE.md` (модульная карта, mapping decision),
`crates/flui-testing/ARCHITECTURE.md`, `docs/testing.md`, комментарии `tasks.rs:1048` и
`Cargo.toml:769-775`.

## Работы

Оценка в инженеро-днях; [P] — параллельно, файлы не пересекаются.

1. **Подмена адаптера, B2 и замер (go/no-go).** `FLUI_TEST_ADAPTER`, `select_backend`,
   `request_flui_device`, минимальный `FrameCapture`, дампы на WARP, железе и lavapipe
   (WSL2), таблица, решение (а)/(б)/(в) или no-go. Нужен WSL2 у мейнтейнера. **4 д.**
2. **Engine-захват.** `FrameCapture` за `testing`, кэш в `render_layer_tree`, `gpu_or_skip`,
   `capture_failure_matrix`, `calibration_detects_broken_conventions` с переключателями.
   Плюс разбор пикселей существующих наборов `gpu-test` после B2: каждое изменение
   объясняется, а не перезаписывается. Файлы `crates/flui-engine/src/{headless,adapter,test_support,lib}.rs`,
   конвейеры для переключателей. После 1. **5.5 д.**
3. [P с 2, 4] **DPR в harness.** `RootSurface`, `MountOptions::physical`, приватные поля и
   миграция вызовов, `lay_out_with`, `mount_root`, `LaidOut::{scene, surface_size}`, R8.
   `crates/flui-testing/src/{bootstrap,realm,widgets}.rs`. **2 д.**
4. [P с 2, 3] **`flui_testing::pixels`.** Сравнение, sidecar, отказ `Update`, glob, R14.
   `crates/flui-testing/src/pixels.rs`, её `Cargo.toml`, модуль в `tests/main.rs`. **3 д.**
5. **Facade `testing-gpu`.** `src/testing/gpu.rs`, `AdapterSummary`, `catch_unwind`, R10,
   снимок поверхности, changelog, `facade-combos`, `TEST_FEATURES`. После 2–4. **2 д.**
6. **Набор Notes.** `tests/gpu_readback/`, перенос composited-теста, RTL-фикстура, точки и
   `samples.txt`, эталоны с ревью каждого PNG, R1/R2/R7–R9, nextest. После 1 и 5. **3.5 д.**
7. **Facade-контроли.** `rewrite`, девять строк D2. После 6. **1.5 д.**
8. [P с 5–7] **ADR и документы.** После 1. **1 д.**
9. [P с 5–7] **`windows-notes`.** xcap/WGC, crop по DWM bounds, `IsIconic`, `--scale`,
   лог-файл пробы, `Damage scissor applied`, `copy_src`, `Selected GPU`, порядок
   `surface_released`/`native_window_destroyed` (trace в
   `crates/flui-platform/src/platforms/windows/platform.rs`, согласуется с `teardown`).
   `tools/xtask/src/device/{windows_notes,uia}.rs`, `tools/xtask/Cargo.toml`. Нужен
   `samples.txt` из 6. **3 д.**
10. [P с 5–9] **`flui-app`.** Строки R12; `closed_window_surface_matrix` в
    `crates/flui-app/tests/main.rs` и шаг запуска. **2.5 д.**
11. **Дефекты, найденные доказательством.** Резерв на исправление того, что поймают замер,
    калибровка и R15 (ожидаемо: края, формат, damage на реальном окне). **4 д.**
12. **Откатные прогоны и циклы ревью** (5 откатов, 3 раунда по 9 PR). **3 д.**
13. **Отчёт и датированный прогон.** Агрегация в `gpu-test`, R16, прогон R14 уровня 0 со
    ссылкой в #1043. После 7, 9–11. **1.5 д.**

**Итого ~36.5 инженеро-дня; критический путь 1→2→5→6→7→11→13 + ревью ≈ 22 дня.** Против
первой оценки (24 д) добавлены подмена адаптера (+1), разбор пикселей после B2 (+2),
engine-переключатели и glyph-строка (+1.5), отказ `Update`/sidecar/glob (+0.5), поверхность,
changelog и combos (+0.5), crop и средние тона (+0.5), ревью PNG (+0.5), резерв на дефекты
(+4), откаты и ревью (+3). Резерв на дефекты — самая неуверенная часть: если замер выберет (а)
и калибровка пройдёт сразу, итог ближе к 32 дням.

Перекрытие с `teardown` (тот же Alt+F4 и `WM_DESTROY`): trace добавляет та, что слита первой.

## Риски

1. **Края между адаптерами и внутри класса.** Если AA-кромки расходятся больше, чем сдвиг на
   ½ px и clip +1 px, срабатывает no-go замера (п. 1): R4 сужается до сравнения внутри класса,
   краевые проверки уходят в engine-калибровку, а snapping ADR-0098 §6 идёт владельцу
   отдельным вопросом. Это меняет объём п. 6–7.
2. **Переход headless на `request_flui_device` и `select_backend()`** меняет пиксели или
   доступность существующих engine-наборов. Смягчение: разбор в п. 2; расхождение — находка,
   её не подгоняют.
3. **Снимок окна.** HDR, Auto Color Management, масштаб DWM или свёрнутое окно портят
   WGC-снимок без участия FLUI. Смягчение: `CANNOT_VERIFY` при advanced color, `IsIconic` и
   несовпадении размера crop.

## Изменения контракта после заморозки (2026-10-06, решение оркестратора)

Основание — отчёт T1 (`render-proof/contract` @ `c679846f3`). Зависимые задачи (T5 — владелец
`pixels.rs`, T10, T14) уведомляются через этот раздел.

1. **Путь обновления эталона.** Замороженный API не давал попытки записи, поэтому
   `reference_update_requires_explicit_opt_in` не мог упасть по задуманной причине. Добавляется
   `Reference::conclude(&self, actual: &Readback, comparison: Comparison, sidecar: &str) ->
   Result<Comparison, ReferenceError>`: в режиме `Verify` возвращает сравнение; в `Update` сначала
   требует, чтобы Size, Samples и калибровка прошли (иначе `Refused`), затем пишет PNG и sidecar и
   возвращает `Updated { path }`. Тест добавляется в T1, реализация — в T5.
2. **`testing-gpu` и CI.** Включение фичи в `TEST_FEATURES` унифицирует `testing` движка и добавляет
   18 тестов, которые паникуют в `test_device_and_queue` без адаптера. Решение: эти тесты проходят
   через то же правило `gpu_or_skip` — без адаптера пропуск, при `FLUI_REQUIRE_GPU=1` жёсткий отказ.
   После этого `testing-gpu` входит в `TEST_FEATURES`, и тест R10 попадает в CI.
3. **`FrameRule::new(channel_tolerance, max_differing_pixels)`** входит в контракт: `FrameRule` —
   `#[non_exhaustive]`, а facade-тестам нужно задавать правило.
4. **`HeadlessRenderer::adapter_info`** сохраняет имя: оно повторяет upstream-тип
   `wgpu::AdapterInfo` и точно его называет (правило именования не нарушено).
