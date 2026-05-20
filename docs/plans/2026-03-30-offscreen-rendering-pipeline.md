# Offscreen Rendering Pipeline — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Подключить существующий OffscreenRenderer к Backend для реализации настоящих GPU эффектов: shader mask, backdrop filter, color matrix, image blur.

**Architecture:** OffscreenRenderer уже реализован (offscreen.rs:288-439 — render_masked с полным pipeline). Нужно: (1) создать OffscreenRenderer в Renderer и передать в Backend, (2) подключить TexturePool к реальным wgpu::Texture, (3) заменить fallback-реализации в Backend на вызовы OffscreenRenderer, (4) интегрировать результат обратно в основной render pass.

**Tech Stack:** wgpu 25.x (Arc<Device>, Arc<Queue>), WGSL shaders (masks/solid.wgsl, masks/linear_gradient.wgsl, masks/radial_gradient.wgsl, effects/blur_*.wgsl), parking_lot::Mutex (TexturePool)

---

## Обзор архитектуры

```
Текущая архитектура:
  Renderer → creates Backend(WgpuPainter) → render_layer_recursive → LayerRender::render/cleanup
  OffscreenRenderer существует но НЕ подключён

Целевая:
  Renderer → creates OffscreenRenderer(Arc<Device>, Arc<Queue>)
           → creates Backend(WgpuPainter, &mut OffscreenRenderer)
           → render_layer_recursive
           → ShaderMaskLayer::render → Backend::render_shader_mask → OffscreenRenderer::render_masked
           → BackdropFilterLayer → Backend::render_backdrop_filter → OffscreenRenderer → blur pipeline
```

**Ключевое ограничение:** `render_shader_mask` в CommandRenderer принимает `&flui_painting::DisplayList`, но ShaderMaskLayer в LayerTree НЕ содержит DisplayList. Дети рисуются через tree traversal. Поэтому:
- Для LayerRender (push/pop pattern) — нужен render-to-offscreen-texture подход
- Для CommandRenderer::render_shader_mask (получает DisplayList) — можно рисовать DisplayList в offscreen и применять маску

---

## Task 1: TexturePool — интеграция с реальными wgpu::Texture

**Проблема:** TexturePool (texture_pool.rs) хранит только дескрипторы (TextureDesc), но не реальные wgpu::Texture. OffscreenRenderer::render_masked() создаёт текстуры вручную (line 382), игнорируя пул.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/texture_pool.rs`
- Test: `crates/flui-engine/src/wgpu/texture_pool.rs` (модуль tests)

**Step 1: Изучить TexturePool**

Прочитать `crates/flui-engine/src/wgpu/texture_pool.rs` полностью. Найти:
- `TexturePoolInner` struct и его поля
- `PooledTexture` struct — что оно хранит
- `acquire()` / `release()` методы
- Закомментированные поля (texture, view)

**Step 2: Добавить wgpu::Texture в PooledTexture**

```rust
pub struct PooledTexture {
    pub desc: TextureDesc,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pool: Arc<Mutex<TexturePoolInner>>,
}
```

**Step 3: Обновить TexturePool::acquire для создания текстур**

```rust
impl TexturePool {
    pub fn acquire(
        &self,
        device: &wgpu::Device,
        size: Size<Pixels>,
        format: wgpu::TextureFormat,
    ) -> PooledTexture {
        let desc = TextureDesc::from_size(size);
        let mut pool = self.inner.lock();

        // Try reuse from pool
        if let Some(cached) = pool.try_reuse(&desc) {
            return cached;
        }

        // Create new texture
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Offscreen Pool Texture"),
            size: wgpu::Extent3d {
                width: desc.width,
                height: desc.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                 | wgpu::TextureUsages::TEXTURE_BINDING
                 | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        PooledTexture { desc, texture, view, pool: Arc::clone(&self.inner) }
    }
}
```

**Step 4: Реализовать Drop для PooledTexture (возврат в пул)**

```rust
impl Drop for PooledTexture {
    fn drop(&mut self) {
        // Return texture to pool for reuse
        // (texture будет перемещена при возврате, нужен Option<wgpu::Texture>)
    }
}
```

> **Примечание:** wgpu::Texture не Clone. Использовать `Option<wgpu::Texture>` + `take()` в Drop, или хранить текстуры в пуле отдельно от PooledTexture handle.

**Step 5: Тесты**

Тесты для TexturePool без GPU можно писать только для descriptor-level логики. GPU-тесты — за `enable-wgpu-tests` feature.

**Step 6: Commit**

```bash
rtk git add crates/flui-engine/src/wgpu/texture_pool.rs
rtk git commit -m "feat(engine): integrate TexturePool with real wgpu::Texture creation"
```

---

## Task 2: OffscreenRenderer — подключить к Renderer

**Проблема:** OffscreenRenderer создаётся нигде. Renderer (renderer.rs) не владеет им.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/renderer.rs:120-129` (добавить поле)
- Modify: `crates/flui-engine/src/wgpu/renderer.rs:220-230` (создать в конструкторе)
- Modify: `crates/flui-engine/src/wgpu/renderer.rs:508-527` (передать в Backend)

**Step 1: Добавить OffscreenRenderer в Renderer struct**

```rust
pub struct Renderer {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface: Option<wgpu::Surface<'static>>,
    config: Option<wgpu::SurfaceConfiguration>,
    capabilities: GpuCapabilities,
    painter: Option<super::painter::WgpuPainter>,
    offscreen: Option<super::offscreen::OffscreenRenderer>,  // NEW
}
```

**Step 2: Создать OffscreenRenderer при инициализации**

В `Renderer::new()` или `configure_surface()`, после создания painter:

```rust
let offscreen = super::offscreen::OffscreenRenderer::new(
    Arc::clone(&device),
    Arc::clone(&queue),
    surface_format,
);
self.offscreen = Some(offscreen);
```

**Step 3: Передать в Backend при рендеринге**

В `render_scene()` (line 508-527):

```rust
if scene.has_content()
    && let Some(painter) = self.painter.take()
{
    let mut backend = if let Some(offscreen) = self.offscreen.as_mut() {
        Backend::with_offscreen(painter, offscreen)
    } else {
        Backend::new(painter)
    };

    // ... render_layer_recursive ...

    let mut painter = backend.into_painter();
    // ...
    self.painter = Some(painter);
}
```

**Step 4: Тесты и commit**

```bash
rtk cargo check -p flui-engine
rtk cargo test -p flui-engine
rtk git add crates/flui-engine/src/wgpu/renderer.rs
rtk git commit -m "feat(engine): create OffscreenRenderer in Renderer and pass to Backend"
```

---

## Task 3: Backend — добавить OffscreenRenderer

**Проблема:** Backend (backend.rs) — thin wrapper вокруг WgpuPainter, без доступа к OffscreenRenderer.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/backend.rs:22-45` (struct + constructors)

**Step 1: Добавить offscreen поле**

```rust
pub struct Backend {
    painter: WgpuPainter,
    offscreen: Option<*mut super::offscreen::OffscreenRenderer>,
}
```

> **Важно:** Нельзя `&mut` из-за lifetime issues с `render_layer_recursive`. Варианты:
> - `Option<&'a mut OffscreenRenderer>` — требует lifetime на Backend (ломает render_layer_recursive)
> - `Option<Arc<Mutex<OffscreenRenderer>>>` — safe, но lock overhead
> - raw pointer — unsafe
>
> **Рекомендация:** `Option<Arc<Mutex<OffscreenRenderer>>>` — безопасно, lock overhead минимален (lock только при shader mask/backdrop, не на каждый frame).

```rust
use std::sync::Arc;
use parking_lot::Mutex;

pub struct Backend {
    painter: WgpuPainter,
    offscreen: Option<Arc<Mutex<super::offscreen::OffscreenRenderer>>>,
}

impl Backend {
    pub fn new(painter: WgpuPainter) -> Self {
        Self { painter, offscreen: None }
    }

    pub fn with_offscreen(
        painter: WgpuPainter,
        offscreen: Arc<Mutex<super::offscreen::OffscreenRenderer>>,
    ) -> Self {
        Self { painter, offscreen: Some(offscreen) }
    }
}
```

**Step 2: Обновить into_painter, painter(), painter_mut()**

Эти методы не меняются — offscreen отбрасывается при `into_painter()`.

**Step 3: Обновить Renderer для Arc<Mutex<OffscreenRenderer>>**

В renderer.rs:
```rust
offscreen: Option<Arc<Mutex<super::offscreen::OffscreenRenderer>>>,

// При создании:
self.offscreen = Some(Arc::new(Mutex::new(offscreen)));

// При передаче:
Backend::with_offscreen(painter, Arc::clone(offscreen_arc))
```

**Step 4: Тесты и commit**

```bash
rtk cargo check -p flui-engine
rtk cargo test -p flui-engine
rtk git add crates/flui-engine/src/wgpu/backend.rs crates/flui-engine/src/wgpu/renderer.rs
rtk git commit -m "feat(engine): add OffscreenRenderer to Backend via Arc<Mutex>"
```

---

## Task 4: render_shader_mask — подключить к OffscreenRenderer

**Проблема:** render_shader_mask (backend.rs:329-356) рисует детей без маски. Нужно: render children → offscreen texture → apply shader mask → composite.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/backend.rs:329-356`

**Step 1: Реализовать render_shader_mask с OffscreenRenderer**

```rust
fn render_shader_mask(
    &mut self,
    child: &flui_painting::DisplayList,
    shader: &flui_painting::Shader,
    bounds: Rect<Pixels>,
    blend_mode: BlendMode,
    _transform: &Matrix4,
) {
    // Try GPU path if OffscreenRenderer available
    if let Some(offscreen_arc) = &self.offscreen {
        let mut offscreen = offscreen_arc.lock();

        // 1. Create offscreen texture for child content
        let child_texture = offscreen.create_child_texture(bounds);

        // 2. Render child DisplayList to offscreen texture
        //    (need a temporary encoder + render pass targeting child_texture)
        // ... this is the complex part ...

        // 3. Apply shader mask
        let shader_spec = ShaderSpec::from_painting_shader(shader);
        let result = offscreen.render_masked(bounds, &shader_spec, blend_mode, &child_texture);

        // 4. Composite result back to main framebuffer
        //    (draw fullscreen quad with result texture)
        self.painter.draw_offscreen_result(&result, bounds);

        return;
    }

    // Fallback: render without mask
    tracing::warn!("ShaderMask: no OffscreenRenderer, rendering child without mask");
    for command in child.commands() {
        dispatch_command(command, self);
    }
}
```

> **Сложность:** Шаг 2 требует рендеринга DisplayList в offscreen texture. Это значит:
> - Создать CommandEncoder
> - Начать render pass с offscreen texture как target
> - Выполнить все команды из DisplayList через dispatch_command
> - Закончить render pass
> - Submit encoder
>
> Проблема: dispatch_command рисует через `self` (Backend), а Backend::painter уже настроен на main target.
> Решение: создать временный WgpuPainter для offscreen target, или добавить метод в WgpuPainter для смены target.

**Step 2: Добавить WgpuPainter::render_to_texture метод**

В painter.rs добавить возможность рендерить в произвольную текстуру:

```rust
impl WgpuPainter {
    /// Flush current batches to a specific texture instead of the screen.
    pub fn render_to_texture(
        &mut self,
        texture_view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), String> {
        // Same as render() but targets texture_view instead of screen
        self.flush_all_instanced_batches(encoder, texture_view);
        self.flush_gradient_batches(encoder, texture_view);
        // ... tessellated batches ...
        // ... text ...
        self.clear_frame_buffers();
        Ok(())
    }
}
```

**Step 3: Добавить WgpuPainter::draw_textured_quad**

Для compositing результата обратно на экран:

```rust
impl WgpuPainter {
    /// Draw a textured quad at the given bounds (for offscreen compositing).
    pub fn draw_textured_quad(
        &mut self,
        texture: &wgpu::Texture,
        bounds: Rect<Pixels>,
        opacity: f32,
    ) {
        // Create texture view + bind group
        // Add to texture_batch for instanced rendering
    }
}
```

**Step 4: Тесты и commit**

```bash
rtk cargo check -p flui-engine
rtk cargo test -p flui-engine
rtk git add crates/flui-engine/src/wgpu/
rtk git commit -m "feat(engine): wire render_shader_mask to OffscreenRenderer GPU pipeline"
```

---

## Task 5: render_backdrop_filter — GPU blur

**Проблема:** Backdrop filter (frosted glass) требует:
1. Copy текущего framebuffer region в offscreen texture
2. Apply Dual Kawase blur (шейдеры уже написаны: blur_downsample.wgsl, blur_upsample.wgsl)
3. Composite blurred result обратно

**Files:**
- Modify: `crates/flui-engine/src/wgpu/backend.rs` (render_backdrop_filter)
- Modify: `crates/flui-engine/src/wgpu/offscreen.rs` (add blur pipeline)
- Modify: `crates/flui-engine/src/wgpu/shaders/effects/blur_downsample.wgsl` (раскомментировать)
- Modify: `crates/flui-engine/src/wgpu/shaders/effects/blur_upsample.wgsl` (раскомментировать)

**Step 1: Раскомментировать blur шейдеры**

Прочитать blur_downsample.wgsl и blur_upsample.wgsl, раскомментировать рабочий код.

**Step 2: Добавить blur pipeline в OffscreenRenderer**

```rust
impl OffscreenRenderer {
    pub fn apply_blur(
        &mut self,
        input_texture: &wgpu::Texture,
        sigma: f32,
        bounds: Rect<Pixels>,
    ) -> PooledTexture {
        let iterations = (sigma / 2.0).ceil() as u32; // Dual Kawase iterations

        // Downsample chain
        let mut current = input_texture;
        let mut mip_chain = Vec::new();
        for i in 0..iterations {
            let mip = self.downsample(current, i);
            mip_chain.push(mip);
            current = &mip_chain.last().unwrap().texture;
        }

        // Upsample chain
        for i in (0..iterations).rev() {
            let upsampled = self.upsample(current, &mip_chain[i]);
            current = upsampled;
        }

        // Return blurred result
        current
    }
}
```

**Step 3: Реализовать render_backdrop_filter**

```rust
fn render_backdrop_filter(
    &mut self,
    child: Option<&DisplayList>,
    filter: &ImageFilter,
    bounds: Rect<Pixels>,
    blend_mode: BlendMode,
    transform: &Matrix4,
) {
    if let Some(offscreen_arc) = &self.offscreen {
        let mut offscreen = offscreen_arc.lock();

        match filter {
            ImageFilter::Blur { sigma_x, sigma_y } => {
                let sigma = (*sigma_x as f32 + *sigma_y as f32) / 2.0;
                // 1. Copy current framebuffer region to texture
                // 2. Apply blur
                // 3. Draw blurred result at bounds
                // 4. Draw child on top
            }
            _ => { /* other filters */ }
        }
        return;
    }

    // Fallback
    if let Some(child) = child {
        for command in child.commands() {
            dispatch_command(command, self);
        }
    }
}
```

> **Сложность:** Копирование framebuffer region требует `wgpu::CommandEncoder::copy_texture_to_texture()`. Нужен доступ к текущему surface texture из render pass. Возможно потребуется передать `encoder` через Backend или сохранить ссылку на текущий surface.

**Step 4: Тесты и commit**

```bash
rtk cargo check -p flui-engine
rtk cargo test -p flui-engine
rtk git add crates/flui-engine/
rtk git commit -m "feat(engine): implement GPU backdrop blur via Dual Kawase pipeline"
```

---

## Task 6: Color matrix — GPU fragment shader

**Проблема:** push_color_filter (backend.rs) использует tint approximation. Нужен реальный 5x4 color matrix shader.

**Files:**
- Create: `crates/flui-engine/src/wgpu/shaders/effects/color_matrix.wgsl`
- Modify: `crates/flui-engine/src/wgpu/offscreen.rs` (add color_matrix pipeline)
- Modify: `crates/flui-engine/src/wgpu/backend.rs` (push/pop_color_filter)

**Step 1: Создать color_matrix.wgsl**

```wgsl
struct ColorMatrixUniforms {
    matrix: mat4x4<f32>,
    offset: vec4<f32>,
}

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(0) @binding(2) var<uniform> params: ColorMatrixUniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    // Fullscreen triangle
    var out: VertexOutput;
    let x = f32(i32(idx & 1u)) * 4.0 - 1.0;
    let y = f32(i32(idx >> 1u)) * 4.0 - 1.0;
    out.position = vec4(x, y, 0.0, 1.0);
    out.uv = vec2((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(input_tex, input_sampler, in.uv);
    // Premultiplied alpha → straight alpha
    let a = max(color.a, 0.001);
    let straight = vec4(color.rgb / a, color.a);
    // Apply 4x4 matrix + offset
    let transformed = params.matrix * straight + params.offset;
    // Clamp and re-premultiply
    let clamped = clamp(transformed, vec4(0.0), vec4(1.0));
    return vec4(clamped.rgb * clamped.a, clamped.a);
}
```

**Step 2: Интегрировать в OffscreenRenderer**

Добавить ShaderType::ColorMatrix, зарегистрировать в shader_compiler.rs, создать pipeline.

**Step 3: Обновить push/pop_color_filter**

push → render-to-offscreen, pop → apply color matrix shader → composite обратно.

Но это требует **encoder access** в push/pop, которого нет. Альтернатива — deferred rendering: запомнить color matrix при push, при pop сделать post-processing pass.

> **Архитектурное ограничение:** push/pop pattern в CommandRenderer не даёт доступ к encoder. Это значит что реальный GPU color matrix невозможен через push/pop. Варианты:
> 1. Изменить CommandRenderer trait (breaking change)
> 2. Сделать deferred — при pop собрать все отрисованные команды и применить фильтр
> 3. Оставить approximation и реализовать полный GPU path только для `render_shader_mask` (который получает DisplayList)

**Step 4: Тесты и commit**

---

## Task 7: Интеграционный тест — end-to-end offscreen pipeline

**Files:**
- Create: `crates/flui-engine/tests/offscreen_integration.rs`

Тест создаёт headless Renderer, рисует DisplayList с shader mask, проверяет что OffscreenRenderer вызван и результат отличается от fallback.

---

## Зависимости

```
Task 1 (TexturePool) ──── не блокирует, но нужен для всех остальных
Task 2 (Renderer + OffscreenRenderer) ──── зависит от Task 1
Task 3 (Backend + OffscreenRenderer) ──── зависит от Task 2
Task 4 (render_shader_mask) ──── зависит от Task 3
Task 5 (backdrop blur) ──── зависит от Task 3 + Task 4 (pattern reuse)
Task 6 (color matrix shader) ──── зависит от Task 3, но ограничен push/pop architecture
Task 7 (integration test) ──── зависит от Task 4
```

**Критический путь:** Task 1 → Task 2 → Task 3 → Task 4 → Task 5

---

## Архитектурные риски

### Risk 1: Encoder access

`WgpuPainter::render()` принимает `&mut CommandEncoder` — но при render_shader_mask мы внутри render_layer_recursive, где encoder уже используется для основного render pass. Нельзя начать новый render pass пока текущий не закончен.

**Mitigation:** OffscreenRenderer создаёт свой собственный `CommandEncoder` и сабмитит отдельно (уже так сделано в render_masked:385-425). Это работает, но порядок GPU operations зависит от порядка submit.

### Risk 2: Framebuffer read для backdrop filter

Backdrop filter требует чтения текущего framebuffer. В wgpu это `copy_texture_to_texture()`, но текущий surface texture может быть занят render pass.

**Mitigation:** Flush painter batches → end current render pass → copy region → apply blur → begin new render pass. Это требует split render pass, что сложно с текущей архитектурой.

### Risk 3: Push/pop pattern vs render-to-texture

Push/pop color filter не имеет доступа к encoder. Полный GPU path невозможен без изменения трейта.

**Mitigation:** Оставить approximation для push/pop, полный GPU path только для render_shader_mask (который получает DisplayList целиком).

---

## Рекомендация по приоритетам

**Реализовать:**
- Task 1-4 (TexturePool → Renderer → Backend → render_shader_mask) — это даёт рабочий GPU shader mask для DisplayList
- Task 7 (integration test) — валидация

**Отложить:**
- Task 5 (backdrop blur) — требует framebuffer copy, split render pass
- Task 6 (color matrix shader) — ограничен push/pop architecture

**Причина:** Tasks 1-4 решают реальную проблему (shader mask) без архитектурных рисков. Tasks 5-6 требуют deeper refactoring (split render pass, trait changes).
