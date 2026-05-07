# flui-engine Completion Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Довести flui-engine до production-ready состояния — реализовать все GPU-заглушки (текст, маски, фильтры), улучшить тесселяцию и интегрировать зарезервированные оптимизации.

**Architecture:** flui-engine — GPU рендер-бэкенд FLUI. Абстрактные трейты `CommandRenderer` (42 метода, visitor) и `Painter` (39 методов) реализуются через wgpu. Команды поступают из `flui-painting::DisplayList`, слои — из `flui-layer::LayerTree`. Рендеринг: instanced batching (rect/circle/arc/gradient/shadow), tessellation (Lyon), text (glyphon).

**Tech Stack:** wgpu 25.x, glyphon 0.9, lyon 1.0, glam 0.30, bytemuck, ttf-parser, WGSL shaders

---

## Phase 1: Текстовый рендеринг (High Priority)

Без текста UI бесполезен. `TextRenderer` в `text.rs` уже работает (add_text → render с glyphon). Но `TextRenderingSystem` в `text_renderer.rs` — заглушка, а `render_text_span` в `backend.rs` не реализован.

### Task 1: Интеграция TextRenderingSystem с TextRenderer

**Проблема:** Два текстовых модуля — `text.rs` (работающий `TextRenderer`) и `text_renderer.rs` (заглушка `TextRenderingSystem`). Нужно унифицировать.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/text_renderer.rs` (весь файл)
- Modify: `crates/flui-engine/src/wgpu/mod.rs` (re-exports)
- Test: `crates/flui-engine/src/wgpu/text_renderer.rs` (модуль tests)

**Step 1: Написать failing тест**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_rendering_system_prepare_returns_valid_run() {
        // TextRun должен сохранять все параметры
        let run = TextRun::new(
            "Hello, FLUI!".to_string(),
            Point::new(DevicePixels(10), DevicePixels(20)),
            TextStyle::default(),
            Color::BLACK,
        );
        assert_eq!(run.text, "Hello, FLUI!");
        assert_eq!(run.position.x, DevicePixels(10));
        assert!(!run.is_empty());
    }

    #[test]
    fn test_text_run_batch_collection() {
        // Проверяем что TextRun можно собирать в батчи
        let runs: Vec<TextRun> = (0..10)
            .map(|i| TextRun::new(
                format!("Line {i}"),
                Point::new(DevicePixels(0), DevicePixels(i * 20)),
                TextStyle::default(),
                Color::BLACK,
            ))
            .collect();
        assert_eq!(runs.len(), 10);
        assert_eq!(runs[5].text, "Line 5");
    }
}
```

**Step 2: Запустить тест — убедиться что проходит**

```bash
rtk cargo test -p flui-engine test_text_rendering_system_prepare -- --nocapture
rtk cargo test -p flui-engine test_text_run_batch -- --nocapture
```

**Step 3: Делегировать TextRenderingSystem → TextRenderer**

В `text_renderer.rs` заменить `render_text_runs` заглушку на делегирование в рабочий `TextRenderer` из `text.rs`:

```rust
/// Render prepared text runs by delegating to the working TextRenderer
pub fn render_text_runs(
    &mut self,
    device: &Device,
    queue: &Queue,
    runs: &[TextRun],
    text_renderer: &mut super::text::TextRenderer,
) {
    for run in runs {
        let font_size = run.style.font_size.unwrap_or(14.0) as f32;
        let position = Point::new(
            Pixels(run.position.x.0 as f32),
            Pixels(run.position.y.0 as f32),
        );
        text_renderer.add_text(&run.text, position, font_size, run.color);
    }
    tracing::trace!(count = runs.len(), "Delegated text runs to TextRenderer");
}
```

**Step 4: Запустить тесты**

```bash
rtk cargo test -p flui-engine -- --nocapture
```

**Step 5: Commit**

```bash
rtk git add crates/flui-engine/src/wgpu/text_renderer.rs
rtk git commit -m "feat(engine): delegate TextRenderingSystem to working TextRenderer"
```

---

### Task 2: Rich text span rendering (render_text_span)

**Проблема:** `backend.rs:213-229` — `render_text_span` логирует warning и ничего не рисует. `InlineSpan` содержит styled segments текста.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/backend.rs:213-229`
- Reference: `crates/flui-types/src/typography/` (InlineSpan definition)
- Test: `crates/flui-engine/src/wgpu/backend.rs` (модуль tests)

**Step 1: Изучить InlineSpan**

```bash
# Найти определение InlineSpan
```

Прочитать `crates/flui-types/src/typography/` чтобы понять структуру `InlineSpan` — какие поля (text, style, children).

**Step 2: Написать failing тест**

Тест должен проверить что `render_text_span` вызывает `text_styled` для каждого сегмента спана. Использовать mock или проверить через painter state.

**Step 3: Реализовать render_text_span**

В `backend.rs` заменить заглушку:

```rust
fn render_text_span(
    &mut self,
    span: &flui_types::typography::InlineSpan,
    offset: Offset<Pixels>,
    _text_scale_factor: f64,
    transform: &Matrix4,
) {
    self.with_transform(transform, |painter| {
        // Рекурсивно отрисовать каждый сегмент спана
        Self::render_span_recursive(painter, span, offset);
    });
}

fn render_span_recursive(
    painter: &mut WgpuPainter,
    span: &InlineSpan,
    offset: Offset<Pixels>,
) {
    match span {
        InlineSpan::Text { text, style } => {
            let font_size = style.font_size.unwrap_or(14.0) as f32;
            let color = style.color.unwrap_or(Color::BLACK);
            let paint = Paint::fill(color);
            let position = Point::new(offset.dx, offset.dy);
            painter.text_styled(text, position, font_size, &paint);
        }
        InlineSpan::Rich { children, .. } => {
            // Lay out children sequentially (simplified — no line breaking)
            let mut current_offset = offset;
            for child in children {
                Self::render_span_recursive(painter, child, current_offset);
                // Advance offset (approximation — proper layout needs text metrics)
            }
        }
    }
}
```

> **Примечание:** Точная реализация зависит от структуры `InlineSpan`. Код выше — шаблон. Изучить тип перед реализацией.

**Step 4: Запустить тесты**

```bash
rtk cargo test -p flui-engine -- --nocapture
rtk cargo check -p flui-engine
```

**Step 5: Commit**

```bash
rtk git add crates/flui-engine/src/wgpu/backend.rs
rtk git commit -m "feat(engine): implement render_text_span for rich text"
```

---

## Phase 2: Shader Mask GPU рендеринг (High Priority)

### Task 3: ShaderMask — wiring OffscreenRenderer в Backend

**Проблема:** `backend.rs:317-344` описывает архитектурное ограничение:
> WgpuRenderer wraps WgpuPainter which doesn't have access to OffscreenRenderer (lives in GpuRenderer).

Три варианта решения (из комментария):
1. Передать OffscreenRenderer в WgpuRenderer constructor
2. Перенести обработку масок на уровень GpuRenderer
3. Дать WgpuPainter доступ к GPU ресурсам

**Рекомендация:** Вариант 1 — наименее инвазивный.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/backend.rs` (Backend struct + render_shader_mask)
- Modify: `crates/flui-engine/src/wgpu/offscreen.rs` (PipelineManager integration)
- Modify: `crates/flui-engine/src/wgpu/layer_render.rs:330-338` (ShaderMaskLayer impl)
- Reference: `crates/flui-engine/src/wgpu/shaders/masks/` (solid.wgsl, linear_gradient.wgsl, radial_gradient.wgsl)

**Step 1: Добавить OffscreenRenderer в Backend**

```rust
pub struct Backend<'a> {
    painter: &'a mut WgpuPainter,
    offscreen: Option<&'a mut OffscreenRenderer>,  // NEW
}

impl<'a> Backend<'a> {
    pub fn new(painter: &'a mut WgpuPainter) -> Self {
        Self { painter, offscreen: None }
    }

    pub fn with_offscreen(
        painter: &'a mut WgpuPainter,
        offscreen: &'a mut OffscreenRenderer,
    ) -> Self {
        Self { painter, offscreen: Some(offscreen) }
    }
}
```

**Step 2: Реализовать render_shader_mask с OffscreenRenderer**

```rust
fn render_shader_mask(
    &mut self,
    child: &flui_painting::DisplayList,
    shader: &flui_painting::Shader,
    bounds: Rect<Pixels>,
    blend_mode: BlendMode,
    transform: &Matrix4,
) {
    if let Some(offscreen) = &mut self.offscreen {
        // 1. Render child to offscreen texture
        // 2. Apply shader mask
        // 3. Composite result back
        offscreen.render_masked(child, shader, bounds, blend_mode);
    } else {
        // Fallback: render without mask
        tracing::warn!("ShaderMask: no OffscreenRenderer, rendering child without mask");
        for command in child.commands() {
            dispatch_command(command, self);
        }
    }
}
```

**Step 3: Реализовать PipelineManager с реальными wgpu pipelines**

В `offscreen.rs:498-541` заменить заглушку:

```rust
pub struct PipelineManager {
    shader_cache: Arc<ShaderCache>,
    device: Arc<wgpu::Device>,
    pipelines: HashMap<ShaderType, wgpu::RenderPipeline>,
}

impl PipelineManager {
    pub fn new(shader_cache: Arc<ShaderCache>, device: Arc<wgpu::Device>) -> Self {
        Self {
            shader_cache,
            device,
            pipelines: HashMap::new(),
        }
    }

    pub fn get_or_create_pipeline(&mut self, shader_type: ShaderType) -> &wgpu::RenderPipeline {
        if !self.pipelines.contains_key(&shader_type) {
            let shader = self.shader_cache.get_or_compile(shader_type);
            let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(shader_type.label()),
                source: wgpu::ShaderSource::Wgsl(shader.source.as_str().into()),
            });
            let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(shader_type.label()),
                layout: None, // Auto layout
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });
            self.pipelines.insert(shader_type, pipeline);
        }
        &self.pipelines[&shader_type]
    }
}
```

**Step 4: Обновить LayerRender для ShaderMaskLayer**

В `layer_render.rs:330-338`:

```rust
impl<R: CommandRenderer + ?Sized> LayerRender<R> for ShaderMaskLayer {
    fn render(&self, renderer: &mut R) {
        // Push shader mask state
        renderer.push_clip_rect(self.bounds());
        // Render children — mask applied via GPU pipeline
    }

    fn cleanup(&self, renderer: &mut R) {
        renderer.pop_clip();
    }
}
```

**Step 5: Тесты и commit**

```bash
rtk cargo check -p flui-engine
rtk cargo test -p flui-engine -- --nocapture
rtk git add crates/flui-engine/src/wgpu/
rtk git commit -m "feat(engine): implement shader mask GPU rendering pipeline"
```

---

### Task 4: Backdrop Filter GPU рендеринг

**Проблема:** `backend.rs:579-607` — backdrop filter (frosted glass, blur) не реализован. Шейдеры `blur_downsample.wgsl` и `blur_upsample.wgsl` уже написаны (закомментированы).

**Files:**
- Modify: `crates/flui-engine/src/wgpu/backend.rs:579-607` (render_backdrop_filter)
- Modify: `crates/flui-engine/src/wgpu/layer_render.rs:340-348` (BackdropFilterLayer impl)
- Modify: `crates/flui-engine/src/wgpu/shaders/effects/blur_downsample.wgsl` (раскомментировать)
- Modify: `crates/flui-engine/src/wgpu/shaders/effects/blur_upsample.wgsl` (раскомментировать)
- Reference: `crates/flui-layer/src/` (BackdropFilterLayer definition)

**Step 1: Раскомментировать шейдеры blur**

В `blur_downsample.wgsl:96-127` и `blur_upsample.wgsl:91-193` раскомментировать реализации Dual Kawase Blur.

**Step 2: Создать BackdropFilterPipeline**

```rust
/// Pipeline for backdrop filter (blur/color effects)
pub struct BackdropFilterPipeline {
    downsample_pipeline: wgpu::ComputePipeline,
    upsample_pipeline: wgpu::ComputePipeline,
    mip_textures: Vec<wgpu::Texture>,
}
```

**Step 3: Реализовать render_backdrop_filter**

```rust
fn render_backdrop_filter(
    &mut self,
    child: Option<&flui_painting::DisplayList>,
    filter: &ImageFilter,
    bounds: Rect<Pixels>,
    _blend_mode: BlendMode,
    transform: &Matrix4,
) {
    self.with_transform(transform, |painter| {
        match filter {
            ImageFilter::Blur { sigma_x, sigma_y } => {
                // 1. Copy backdrop region to offscreen texture
                // 2. Apply Dual Kawase blur (downsample → upsample chain)
                // 3. Composite blurred backdrop back
                painter.apply_backdrop_blur(bounds, *sigma_x, *sigma_y);
            }
            _ => {
                tracing::warn!("Unsupported backdrop filter type");
            }
        }

        // 4. Render child content on top
        if let Some(child) = child {
            for command in child.commands() {
                dispatch_command(command, &mut *painter_as_backend);
            }
        }
    });
}
```

> **Примечание:** Backdrop filter требует чтения текущего framebuffer. Это сложнее масок — нужен copy-to-texture + compute/fragment pass. Реализация зависит от доступа к wgpu encoder в контексте рендеринга.

**Step 4: Обновить LayerRender для BackdropFilterLayer**

```rust
impl<R: CommandRenderer + ?Sized> LayerRender<R> for BackdropFilterLayer {
    fn render(&self, renderer: &mut R) {
        renderer.render_backdrop_filter(
            None,
            self.filter(),
            self.bounds(),
            self.blend_mode(),
            &Matrix4::IDENTITY,
        );
    }

    fn cleanup(&self, _renderer: &mut R) {
        // Backdrop filter is stateless — no cleanup needed
    }
}
```

**Step 5: Тесты и commit**

```bash
rtk cargo check -p flui-engine
rtk cargo test -p flui-engine -- --nocapture
rtk git add crates/flui-engine/
rtk git commit -m "feat(engine): implement backdrop filter with Dual Kawase blur"
```

---

## Phase 3: GPU фильтры (Medium Priority)

### Task 5: Color filter через GPU shader

**Проблема:** `backend.rs:716-725` — `push_color_filter` делает save/restore, но не применяет цветовую матрицу.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/backend.rs:716-725`
- Create: `crates/flui-engine/src/wgpu/shaders/effects/color_filter.wgsl` (если нет)
- Test: unit test для color matrix application

**Step 1: Написать WGSL шейдер для color matrix**

```wgsl
// color_filter.wgsl
struct ColorMatrix {
    m: mat4x4<f32>,   // 4x4 color transformation
    offset: vec4<f32>, // Color offset
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(0) @binding(2) var<uniform> matrix: ColorMatrix;

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let color = textureSample(input_texture, input_sampler, uv);
    let transformed = matrix.m * color + matrix.offset;
    return clamp(transformed, vec4(0.0), vec4(1.0));
}
```

**Step 2: Реализовать push/pop color filter**

Подход: render-to-texture → apply color matrix shader → composite back.

```rust
fn push_color_filter(&mut self, filter: &ColorMatrix) {
    // Save current render target
    self.painter.save();
    // Begin offscreen rendering for color filter
    self.painter.begin_color_filter(filter);
}

fn pop_color_filter(&mut self) {
    // Apply color matrix shader and composite
    self.painter.end_color_filter();
    self.painter.restore();
}
```

**Step 3: Тесты и commit**

```bash
rtk cargo check -p flui-engine
rtk cargo test -p flui-engine -- --nocapture
rtk git add crates/flui-engine/
rtk git commit -m "feat(engine): implement color filter via GPU color matrix shader"
```

---

### Task 6: Image filter (blur/dilate/erode) через compute shader

**Проблема:** `backend.rs:728` — `push_image_filter` не реализован.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/backend.rs:726-735`
- Reference: `crates/flui-engine/src/wgpu/shaders/effects/blur_*.wgsl`

**Step 1: Реализовать push/pop image filter**

Аналогично color filter — render-to-texture → apply filter → composite. Для blur использовать уже готовые шейдеры (blur_downsample + blur_upsample).

**Step 2: Тесты и commit**

```bash
rtk cargo check -p flui-engine
rtk cargo test -p flui-engine -- --nocapture
rtk git add crates/flui-engine/
rtk git commit -m "feat(engine): implement image filter via GPU compute shaders"
```

---

## Phase 4: Тесселяция и геометрия (Medium Priority)

### Task 7: Per-corner border radius

**Проблема:** `tessellator.rs:536` — RRect рисуется с усреднённым радиусом вместо per-corner.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/tessellator.rs:530-545`
- Test: `crates/flui-engine/src/wgpu/tessellator.rs` (модуль tests)

**Step 1: Написать failing тест**

```rust
#[test]
fn test_rrect_per_corner_radii() {
    let mut t = Tessellator::new();
    let rrect = RRect {
        rect: Rect::from_ltwh(0.0, 0.0, 100.0, 100.0),
        top_left: BorderRadius::circular(10.0),
        top_right: BorderRadius::circular(20.0),
        bottom_right: BorderRadius::circular(5.0),
        bottom_left: BorderRadius::circular(0.0),
    };
    let paint = Paint::fill(Color::RED);
    let (vertices, indices) = t.tessellate_rrect(&rrect, &paint).unwrap();
    // Вершины должны содержать разные радиусы для каждого угла
    assert!(!vertices.is_empty());
    assert!(!indices.is_empty());
}
```

**Step 2: Запустить — убедиться что тест проходит с текущим кодом (но не проверяет корректность)**

```bash
rtk cargo test -p flui-engine test_rrect_per_corner -- --nocapture
```

**Step 3: Реализовать per-corner radii**

Заменить усреднение радиуса на lyon path builder с разными радиусами:

```rust
// Вместо:
let radius = (rrect.top_left.x + ... ) / 8.0;

// Использовать:
let mut builder = Path::builder();
// Top-left corner arc
builder.begin(point(rect.x + tl_radius, rect.y));
// Top edge
builder.line_to(point(rect.right() - tr_radius, rect.y));
// Top-right corner arc
builder.quadratic_bezier_to(
    point(rect.right(), rect.y),
    point(rect.right(), rect.y + tr_radius),
);
// ... repeat for each corner with its own radius
builder.close();
```

**Step 4: Тесты и commit**

```bash
rtk cargo test -p flui-engine -- --nocapture
rtk git add crates/flui-engine/src/wgpu/tessellator.rs
rtk git commit -m "feat(engine): support per-corner border radii in RRect tessellation"
```

---

### Task 8: Улучшение arc tessellation

**Проблема:** `painter.rs:1440` — дуги тесселируются приблизительно, TODO на улучшение.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/tessellator.rs` (tessellate_arc method)
- Test: тесты для дуг с разными sweep angles

**Step 1: Написать тесты для edge cases**

```rust
#[test]
fn test_arc_full_circle() { /* sweep_angle = 2π */ }

#[test]
fn test_arc_semicircle() { /* sweep_angle = π */ }

#[test]
fn test_arc_quarter() { /* sweep_angle = π/2 */ }

#[test]
fn test_arc_negative_sweep() { /* sweep_angle < 0 */ }
```

**Step 2: Улучшить тесселяцию — использовать lyon arc**

Заменить ручную аппроксимацию на `lyon::path::builder::SvgPathBuilder::arc_to()` для точных кривых Безье.

**Step 3: Тесты и commit**

```bash
rtk cargo test -p flui-engine test_arc -- --nocapture
rtk git add crates/flui-engine/src/wgpu/tessellator.rs
rtk git commit -m "fix(engine): improve arc tessellation using lyon arc primitives"
```

---

## Phase 5: Shader кеширование и Pipeline (Medium Priority)

### Task 9: ShaderCompiler — кешировать wgpu::ShaderModule

**Проблема:** `shader_compiler.rs:73` — `CompiledShader` хранит только source string, но не `wgpu::ShaderModule`. Каждый раз шейдер компилируется заново.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/shader_compiler.rs:70-125`
- Test: `crates/flui-engine/src/wgpu/shader_compiler.rs` (модуль tests)

**Step 1: Добавить wgpu::ShaderModule в CompiledShader**

```rust
pub struct CompiledShader {
    pub shader_type: ShaderType,
    pub source: String,
    pub module: Option<Arc<wgpu::ShaderModule>>,  // Cached GPU module
}
```

**Step 2: Добавить метод compile_to_module**

```rust
impl ShaderCache {
    /// Compile shader and cache the wgpu::ShaderModule
    pub fn get_or_compile_module(
        &self,
        shader_type: ShaderType,
        device: &wgpu::Device,
    ) -> Arc<wgpu::ShaderModule> {
        // Double-check locking for thread safety
        // Create module via device.create_shader_module()
        // Cache in CompiledShader.module
    }
}
```

**Step 3: Тесты и commit**

```bash
rtk cargo test -p flui-engine shader -- --nocapture
rtk git add crates/flui-engine/src/wgpu/shader_compiler.rs
rtk git commit -m "feat(engine): cache wgpu::ShaderModule in ShaderCache"
```

---

### Task 10: Pipeline key tracking для батчинга

**Проблема:** `painter.rs:771` — pipeline key не отслеживается per draw call, что мешает оптимальному батчингу.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/painter.rs:770-780`
- Modify: `crates/flui-engine/src/wgpu/pipelines.rs` (PipelineKey usage)

**Step 1: Добавить pipeline key tracking**

```rust
// В каждом draw call:
fn draw_rect_internal(&mut self, ...) {
    let key = PipelineKey::for_rect(blend_mode, has_texture);
    if Some(key) != self.current_pipeline_key {
        self.flush_batch(); // Flush current batch before switching pipeline
        self.current_pipeline_key = Some(key);
    }
    self.rect_batch.push(instance);
}
```

**Step 2: Тесты и commit**

```bash
rtk cargo test -p flui-engine -- --nocapture
rtk git add crates/flui-engine/src/wgpu/painter.rs crates/flui-engine/src/wgpu/pipelines.rs
rtk git commit -m "feat(engine): track pipeline key per draw call for optimal batching"
```

---

## Phase 6: Тесты (Medium Priority)

### Task 11: Тесты для layer_render.rs

**Проблема:** `layer_render.rs` — критичный dispatch код без тестов.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/layer_render.rs` (добавить модуль tests)

**Step 1: Создать mock CommandRenderer**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Mock renderer that records which methods were called
    struct MockRenderer {
        calls: Vec<String>,
    }

    impl MockRenderer {
        fn new() -> Self { Self { calls: Vec::new() } }
    }

    impl CommandRenderer for MockRenderer {
        fn render_rect(&mut self, ..) { self.calls.push("render_rect".into()); }
        fn push_offset(&mut self, ..) { self.calls.push("push_offset".into()); }
        fn pop_transform(&mut self) { self.calls.push("pop_transform".into()); }
        fn push_opacity(&mut self, ..) { self.calls.push("push_opacity".into()); }
        fn pop_opacity(&mut self) { self.calls.push("pop_opacity".into()); }
        fn push_clip_rect(&mut self, ..) { self.calls.push("push_clip_rect".into()); }
        fn pop_clip(&mut self) { self.calls.push("pop_clip".into()); }
        // ... остальные методы — no-op
    }
}
```

**Step 2: Тесты для каждого типа слоя**

```rust
#[test]
fn test_offset_layer_render_calls_push_pop() {
    let mut renderer = MockRenderer::new();
    let layer = OffsetLayer::new(Offset::new(10.0, 20.0));
    layer.render(&mut renderer);
    assert!(renderer.calls.contains(&"push_offset".into()));
    layer.cleanup(&mut renderer);
    assert!(renderer.calls.contains(&"pop_transform".into()));
}

#[test]
fn test_opacity_layer_render() { /* ... */ }

#[test]
fn test_clip_rect_layer_render() { /* ... */ }
```

**Step 3: Тесты и commit**

```bash
rtk cargo test -p flui-engine layer_render -- --nocapture
rtk git add crates/flui-engine/src/wgpu/layer_render.rs
rtk git commit -m "test(engine): add unit tests for LayerRender dispatch"
```

---

### Task 12: Тесты для VectorTextRenderer

**Files:**
- Modify: `crates/flui-engine/src/utils/text.rs` (добавить модуль tests)

**Step 1: Написать тесты**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_text_basic_ascii() {
        let renderer = VectorTextRenderer::new();
        let paths = renderer.text_to_paths("Hello", 16.0);
        assert!(!paths.is_empty(), "ASCII text should produce paths");
    }

    #[test]
    fn test_vector_text_empty_string() {
        let renderer = VectorTextRenderer::new();
        let paths = renderer.text_to_paths("", 16.0);
        assert!(paths.is_empty());
    }
}
```

**Step 2: Тесты и commit**

```bash
rtk cargo test -p flui-engine vector_text -- --nocapture
rtk git add crates/flui-engine/src/utils/text.rs
rtk git commit -m "test(engine): add unit tests for VectorTextRenderer"
```

---

## Phase 7: Оптимизации (Low Priority)

### Task 13: Активировать multi_draw.rs

**Files:**
- Modify: `crates/flui-engine/src/wgpu/multi_draw.rs` (remove `#[allow(dead_code)]`)
- Modify: `crates/flui-engine/src/wgpu/painter.rs` (integrate MultiDrawBatcher)

Интегрировать `MultiDrawBatcher` в flush-path `WgpuPainter` для indirect draw calls. Это снижает CPU overhead за счёт пакетной отправки draw commands на GPU.

**Step 1: Написать benchmark/тест для батчинга**

**Step 2: Интегрировать в painter flush**

**Step 3: Тесты и commit**

---

### Task 14: Активировать instancing и buffer_pool

**Files:**
- Modify: `crates/flui-engine/src/wgpu/instancing.rs`
- Modify: `crates/flui-engine/src/wgpu/buffer_pool.rs`
- Modify: `crates/flui-engine/src/wgpu/painter.rs`

> **Примечание:** instancing уже частично используется painter (rect_batch, circle_batch). Задача — активировать buffer pooling для переиспользования GPU буферов между кадрами.

**Step 1-3:** Аналогично Task 13.

---

### Task 15: Платформенные оптимизации (Metal/DX12/Vulkan)

**Files:**
- Modify: `crates/flui-engine/src/wgpu/metal.rs` (MetalFX upscaling, HDR)
- Modify: `crates/flui-engine/src/wgpu/dx12.rs` (DirectStorage, capabilities)
- Modify: `crates/flui-engine/src/wgpu/vulkan.rs` (pipeline cache, driver detection)

Реализовать по мере необходимости. Каждый файл содержит подробные TODO с описанием нужной функциональности.

---

## Зависимости между задачами

```
Task 1 (TextRenderingSystem) ─── не блокирует
Task 2 (Rich text span) ──────── зависит от Task 1
Task 3 (Shader Mask) ─────────── не блокирует
Task 4 (Backdrop Filter) ─────── зависит от Task 3 (shared OffscreenRenderer)
Task 5 (Color filter) ────────── зависит от Task 3 (offscreen pattern)
Task 6 (Image filter) ────────── зависит от Task 4 (blur shaders)
Task 7 (Per-corner radii) ────── не блокирует
Task 8 (Arc tessellation) ────── не блокирует
Task 9 (Shader caching) ─────── не блокирует, но полезен для Task 3
Task 10 (Pipeline batching) ──── зависит от Task 9
Task 11 (Tests layer_render) ─── не блокирует
Task 12 (Tests vector text) ──── не блокирует
Task 13 (multi_draw) ─────────── зависит от Task 10
Task 14 (buffer_pool) ────────── не блокирует
Task 15 (Platform) ───────────── не блокирует
```

**Параллельные потоки работы:**
- **Поток A (Text):** Task 1 → Task 2
- **Поток B (Effects):** Task 9 → Task 3 → Task 4 → Task 5, Task 6
- **Поток C (Geometry):** Task 7, Task 8 (параллельно)
- **Поток D (Tests):** Task 11, Task 12 (параллельно, в любое время)
- **Поток E (Optimization):** Task 10 → Task 13, Task 14 (после Phase 1-4)

---

## Оценка объёма

| Phase | Tasks | Сложность |
|-------|-------|-----------|
| Phase 1: Текст | 1-2 | Средняя |
| Phase 2: Shader Mask + Backdrop | 3-4 | Высокая (GPU pipeline wiring) |
| Phase 3: Фильтры | 5-6 | Средняя |
| Phase 4: Тесселяция | 7-8 | Низкая |
| Phase 5: Shader/Pipeline | 9-10 | Средняя |
| Phase 6: Тесты | 11-12 | Низкая |
| Phase 7: Оптимизации | 13-15 | Средняя-Высокая |
