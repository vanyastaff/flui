//! Pipeline and uniform layout for the per-pixel color-matrix filter pass.
//!
//! [`ColorMatrixPipeline`] owns the `wgpu::RenderPipeline` used by
//! [`super::apply_color_matrix`].  It is format-parametric so the same pipeline
//! serves both the windowed surface format and any pooled offscreen target format.
//!
//! ## Matrix upload format
//!
//! The CPU-side color matrix is stored row-major as `[f32; 20]`:
//!
//! ```text
//!   [r0, r1, r2, r3, offset_r,   // row 0: R output weights + bias
//!    g0, g1, g2, g3, offset_g,   // row 1: G output weights + bias
//!    b0, b1, b2, b3, offset_b,   // row 2: B output weights + bias
//!    a0, a1, a2, a3, offset_a]   // row 3: A output weights + bias
//! ```
//!
//! `ColorMatrixUniform::from_values` splits this into a 4×4 weight matrix and a
//! vec4 offset, uploaded as a single 80-byte uniform block.
//!
//! ## WGSL column-major layout — critical correctness note
//!
//! WGSL `mat4x4<f32>` is **column-major**: the first 16 floats in memory become
//! column 0, column 1, column 2, column 3 of the matrix.  `mat * vec` in WGSL
//! is the standard column-major product: `result[i] = dot(column_i, vec)`.
//!
//! We want `result[i] = dot(row_i_of_M, straight)` (the row dot-product).
//! To achieve this we pack the **columns** of M into the four `[f32;4]` blocks
//! so that WGSL's `u.m * straight` computes exactly the row dot-products:
//!
//! ```text
//!   m[0] = [r0, g0, b0, a0]  (column 0 of M: first  weight of every row)
//!   m[1] = [r1, g1, b1, a1]  (column 1 of M: second weight of every row)
//!   m[2] = [r2, g2, b2, a2]  (column 2 of M: third  weight of every row)
//!   m[3] = [r3, g3, b3, a3]  (column 3 of M: fourth weight of every row)
//! ```
//!
//! WGSL then sees `result[i] = Σ_j column_j[i] * straight[j]
//!                            = Σ_j M[i,j] * straight[j]`  ✓

use std::mem;

use bytemuck::{Pod, Zeroable};

// ── Uniform layout (mirrors `ColorMatrixUniforms` in color_matrix.wgsl) ──────

/// GPU uniform buffer layout for the color-matrix filter pass.
///
/// **Explicit `#[repr(C)]` layout — must match `ColorMatrixUniforms` in
/// `color_matrix.wgsl` byte-for-byte.**
///
/// WGSL uniform-block alignment rules (WGSL §13.4.1, WebGPU §3.13.3):
/// - `mat4x4<f32>` → align 16, size 64
/// - `vec4<f32>`   → align 16, size 16
///
/// | Byte offset | Size | Rust field | WGSL member     |
/// |-------------|------|------------|-----------------|
/// | 0           | 64   | `m`        | `mat4x4<f32>`   |
/// | 64          | 16   | `offset`   | `vec4<f32>`     |
///
/// Total = 80 bytes (multiple of 16 ✓).
///
/// `m[i]` is **column `i`** of the 4×4 weight matrix.  WGSL `mat4x4<f32>`
/// is column-major, so `u.m * straight` computes `result[row] = dot(M_row, straight)`,
/// the intended row-major matrix–vector product.  See crate-level doc comment for
/// the full layout derivation.
/// `offset[i]` is the additive bias for output channel `i` (R/G/B/A).
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub(super) struct ColorMatrixUniform {
    /// 4×4 weight matrix stored as its four COLUMNS.
    ///
    /// WGSL `mat4x4<f32>` reads consecutive floats as columns; storing column `i`
    /// in `m[i]` makes `u.m` equal to M (not Mᵀ), so `u.m * straight` gives the
    /// correct row dot-products.
    pub(super) m: [[f32; 4]; 4],
    /// Additive offset/bias for each output channel (R/G/B/A).
    pub(super) offset: [f32; 4],
}

const _COLOR_MATRIX_UNIFORM_SIZE_CHECK: () = {
    assert!(mem::size_of::<ColorMatrixUniform>() == 80);
};

impl ColorMatrixUniform {
    /// Build a uniform from the flat `[f32; 20]` row-major color-matrix layout.
    ///
    /// Input layout: `[r0, r1, r2, r3, off_r,  g0..off_g,  b0..off_b,  a0..off_a]`.
    ///
    /// Each `m[i]` is stored as **column `i`** of the 4×4 weight matrix so that
    /// WGSL's column-major `mat4x4<f32>` reads `u.m` as M (not Mᵀ).  The mapping:
    ///
    /// ```text
    ///   m[0] = [r0, g0, b0, a0]  column 0: first  weight of every row
    ///   m[1] = [r1, g1, b1, a1]  column 1: second weight of every row
    ///   m[2] = [r2, g2, b2, a2]  column 2: third  weight of every row
    ///   m[3] = [r3, g3, b3, a3]  column 3: fourth weight of every row
    /// ```
    pub(crate) fn from_values(values: [f32; 20]) -> Self {
        Self {
            m: [
                // Column 0: the first RGBA weight of each output channel.
                [values[0], values[5], values[10], values[15]],
                // Column 1: the second RGBA weight of each output channel.
                [values[1], values[6], values[11], values[16]],
                // Column 2: the third RGBA weight of each output channel.
                [values[2], values[7], values[12], values[17]],
                // Column 3: the fourth RGBA weight of each output channel.
                [values[3], values[8], values[13], values[18]],
            ],
            offset: [values[4], values[9], values[14], values[19]],
        }
    }
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

/// Bind-group layout and render pipeline for the color-matrix filter pass.
///
/// ## Bind-group layout (group 0)
///
/// | Binding | Stage | Type                  | Content                      |
/// |---------|-------|-----------------------|------------------------------|
/// | 0       | FS    | Uniform buffer        | [`ColorMatrixUniform`]       |
/// | 1       | FS    | 2D float texture      | Source layer (premultiplied) |
/// | 2       | FS    | Non-filtering sampler | Nearest + ClampToEdge        |
///
/// ## Blend state
///
/// `REPLACE` (no fixed-function blending): the fragment shader emits the full
/// premultiplied filtered texel directly; the GPU hardware must not re-blend it.
#[allow(missing_debug_implementations)]
pub(crate) struct ColorMatrixPipeline {
    /// Bind-group layout shared between pipeline construction and per-draw
    /// bind-group creation in [`super::apply_color_matrix`].
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
    /// The single render pipeline (format-parametric at construction time).
    pub(crate) pipeline: wgpu::RenderPipeline,
}

impl ColorMatrixPipeline {
    /// Build the pipeline for `surface_format`.
    ///
    /// WGSL shader compilation happens here; a wgpu validation error surfaces
    /// at this call site, which the GPU construction test exercises.
    pub(crate) fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = create_bind_group_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Color Matrix Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Color Matrix Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/effects/color_matrix.wgsl").into(),
            ),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Color Matrix Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                // No vertex buffers: the VS synthesises 6 vertices from
                // `@builtin(vertex_index)`, forming a full-viewport quad.
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    // REPLACE: the shader emits the full premultiplied result.
                    // Fixed-function blending must not re-blend it.
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        Self {
            bind_group_layout,
            pipeline,
        }
    }
}

// ── Bind-group layout factory (private) ──────────────────────────────────────

fn create_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Color Matrix Bind Group Layout"),
        entries: &[
            // Binding 0: uniform buffer (FS)
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(
                        mem::size_of::<ColorMatrixUniform>() as u64
                    ),
                },
                count: None,
            },
            // Binding 1: source layer texture (FS) — non-filtering (Nearest)
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            // Binding 2: nearest-clamp sampler (FS)
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                count: None,
            },
        ],
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
// `float_cmp` fires on `assert_eq!` of f32 arrays; these tests verify exact
// bit-identity of literals stored and read back through a struct — no arithmetic,
// so equality is bit-exact and the comparison is correct.
#[allow(clippy::float_cmp)]
mod cpu_tests {
    use super::ColorMatrixUniform;

    /// The struct size must match the WGSL uniform-block size exactly.
    ///
    /// Layout: m(64) + offset(16) = 80 bytes.
    #[test]
    fn color_matrix_uniform_size_is_80_bytes() {
        assert_eq!(
            std::mem::size_of::<ColorMatrixUniform>(),
            80,
            "ColorMatrixUniform must be 80 bytes to match the WGSL ColorMatrixUniforms struct"
        );
    }

    /// `from_values` splits the flat 20-element layout correctly.
    ///
    /// The identity matrix in [f32;20] form:
    /// ```text
    ///   [1,0,0,0,0,  0,1,0,0,0,  0,0,1,0,0,  0,0,0,1,0]
    /// ```
    /// must produce `m[i] = column i of I₄` (= I₄ itself) and `offset = [0;4]`.
    ///
    /// For identity: column 0 = (1,0,0,0), column 1 = (0,1,0,0), etc.
    #[test]
    fn from_values_identity_matrix() {
        #[rustfmt::skip]
        let identity: [f32; 20] = [
            1.0, 0.0, 0.0, 0.0, 0.0,  // R row: weight R=1, G=0, B=0, A=0, off=0
            0.0, 1.0, 0.0, 0.0, 0.0,  // G row
            0.0, 0.0, 1.0, 0.0, 0.0,  // B row
            0.0, 0.0, 0.0, 1.0, 0.0,  // A row
        ];
        let u = ColorMatrixUniform::from_values(identity);
        // m[i] is column i — for the identity, columns equal the standard basis.
        assert_eq!(u.m[0], [1.0, 0.0, 0.0, 0.0], "column 0: (r0,g0,b0,a0)");
        assert_eq!(u.m[1], [0.0, 1.0, 0.0, 0.0], "column 1: (r1,g1,b1,a1)");
        assert_eq!(u.m[2], [0.0, 0.0, 1.0, 0.0], "column 2: (r2,g2,b2,a2)");
        assert_eq!(u.m[3], [0.0, 0.0, 0.0, 1.0], "column 3: (r3,g3,b3,a3)");
        assert_eq!(u.offset, [0.0, 0.0, 0.0, 0.0], "offset");
    }

    /// `from_values` extracts offsets from column 4 of each row.
    #[test]
    fn from_values_extracts_offset_correctly() {
        #[rustfmt::skip]
        let values: [f32; 20] = [
            1.0, 0.0, 0.0, 0.0, 0.1,  // off_r = 0.1
            0.0, 1.0, 0.0, 0.0, 0.2,  // off_g = 0.2
            0.0, 0.0, 1.0, 0.0, 0.3,  // off_b = 0.3
            0.0, 0.0, 0.0, 1.0, 0.4,  // off_a = 0.4
        ];
        let u = ColorMatrixUniform::from_values(values);
        assert_eq!(
            u.offset,
            [0.1, 0.2, 0.3, 0.4],
            "offsets from column 4 of each row"
        );
    }

    /// `from_values` maps a swap-R/B matrix into the correct columns.
    ///
    /// The "swap R and B channels" matrix (used in the GPU readback tests):
    /// ```text
    ///   row 0 (R_out): [0,0,1,0,0]   R_out = B_in
    ///   row 1 (G_out): [0,1,0,0,0]   G_out = G_in
    ///   row 2 (B_out): [1,0,0,0,0]   B_out = R_in
    ///   row 3 (A_out): [0,0,0,1,0]   A_out = A_in
    /// ```
    /// Columns of this matrix:
    ///   column 0: (0,0,1,0) — the R-input weight for every output channel
    ///   column 1: (0,1,0,0)
    ///   column 2: (1,0,0,0)
    ///   column 3: (0,0,0,1)
    #[test]
    fn from_values_swap_rb_matrix() {
        #[rustfmt::skip]
        let swap_rb: [f32; 20] = [
            0.0, 0.0, 1.0, 0.0, 0.0,  // R_out = B_in
            0.0, 1.0, 0.0, 0.0, 0.0,  // G_out = G_in
            1.0, 0.0, 0.0, 0.0, 0.0,  // B_out = R_in
            0.0, 0.0, 0.0, 1.0, 0.0,  // A_out = A_in
        ];
        let u = ColorMatrixUniform::from_values(swap_rb);
        // m[i] is column i of the swap-R/B matrix.
        assert_eq!(
            u.m[0],
            [0.0, 0.0, 1.0, 0.0],
            "column 0: R-input weight per output row"
        );
        assert_eq!(
            u.m[1],
            [0.0, 1.0, 0.0, 0.0],
            "column 1: G-input weight per output row"
        );
        assert_eq!(
            u.m[2],
            [1.0, 0.0, 0.0, 0.0],
            "column 2: B-input weight per output row"
        );
        assert_eq!(
            u.m[3],
            [0.0, 0.0, 0.0, 1.0],
            "column 3: A-input weight per output row"
        );
    }

    /// Asymmetric matrix + offset: `from_values` → uniform → CPU oracle must agree.
    ///
    /// Uses a matrix with no symmetry so the transpose bug would produce wrong values.
    /// Applies it to a non-trivial RGBA color and compares the uniform-computed result
    /// with `ColorMatrix::apply`.
    #[test]
    fn from_values_asymmetric_matrix_with_offset_matches_apply() {
        use flui_types::painting::ColorMatrix;
        // Asymmetric 5×4 matrix: off-diagonal weights differ above/below diagonal.
        #[rustfmt::skip]
        let values: [f32; 20] = [
            0.6, 0.2, 0.1, 0.0, 0.05,  // R_out
            0.1, 0.7, 0.05, 0.0, 0.0,  // G_out
            0.05, 0.1, 0.8, 0.0, 0.0,  // B_out
            0.0, 0.0, 0.0, 1.0, 0.0,   // A_out
        ];
        let matrix = ColorMatrix::new(values);
        let u = ColorMatrixUniform::from_values(values);

        // Verify that applying the uniform reproduces ColorMatrix::apply for a
        // straight-alpha input color — the CPU oracle for the shader's math.
        let straight = [0.8_f32, 0.4, 0.2, 1.0]; // opaque warm orange
        let cpu_result = matrix.apply(straight);

        // Reproduce the uniform's dot-products: result[i] = Σ_j u.m[j][i] * straight[j]
        // (column-major mat*vec: column j contributes column[j]*straight[j]).
        let gpu_result: [f32; 4] = std::array::from_fn(|output_channel| {
            (0..4)
                .map(|input_channel| u.m[input_channel][output_channel] * straight[input_channel])
                .sum::<f32>()
                + u.offset[output_channel]
        });

        for ch in 0..4 {
            assert!(
                (gpu_result[ch] - cpu_result[ch]).abs() < 1e-5,
                "channel {ch}: uniform-computed={} cpu-oracle={}",
                gpu_result[ch],
                cpu_result[ch]
            );
        }
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_construction_tests {
    use super::ColorMatrixPipeline;

    fn test_device() -> wgpu::Device {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter must be available on a GPU-enabled test host");
        let (device, _queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("ColorMatrixPipeline Test Device"),
                ..Default::default()
            }))
            .expect("GPU device creation succeeded when adapter was found");
        device
    }

    /// `ColorMatrixPipeline::new` completes without a wgpu validation error for
    /// `Rgba8Unorm`, proving the WGSL parses and the bind-group layout is valid.
    #[test]
    fn pipeline_construction_succeeds_for_rgba8unorm() {
        let device = test_device();
        let _pipeline = ColorMatrixPipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    }
}
