//! Effects batching (gradients, shadows, blur).
//!
//! Accumulates gradient, shadow, and blur effect instances for GPU submission.

// ---------------------------------------------------------------------------
// Instance types
// ---------------------------------------------------------------------------

/// A single color stop within a gradient.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GradientStop {
    /// Stop color (RGBA).
    pub color: [f32; 4],
    /// Normalized position along the gradient `[0.0, 1.0]`.
    pub position: f32,
    /// Padding to align to 32 bytes.
    pub _padding: [f32; 3],
}

/// A linear gradient effect applied to a bounded region.
#[derive(Clone, Debug)]
pub struct LinearGradientInstance {
    /// Bounding rectangle (x, y, w, h).
    pub bounds: [f32; 4],
    /// Gradient start point.
    pub start: [f32; 2],
    /// Gradient end point.
    pub end: [f32; 2],
    /// Color stops along the gradient.
    pub stops: Vec<GradientStop>,
    /// Per-corner radii for rounded-rect gradients.
    pub corner_radii: [f32; 4],
    /// 2×2 affine transform packed as `[a, b, c, d]`.
    pub transform: [f32; 4],
}

/// A radial gradient effect applied to a bounded region.
#[derive(Clone, Debug)]
pub struct RadialGradientInstance {
    /// Bounding rectangle (x, y, w, h).
    pub bounds: [f32; 4],
    /// Center of the radial gradient.
    pub center: [f32; 2],
    /// Radius of the gradient.
    pub radius: f32,
    /// Color stops along the gradient.
    pub stops: Vec<GradientStop>,
    /// Per-corner radii for rounded-rect gradients.
    pub corner_radii: [f32; 4],
    /// 2×2 affine transform packed as `[a, b, c, d]`.
    pub transform: [f32; 4],
}

/// A box-shadow effect instance.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShadowInstance {
    /// Shadow bounding rectangle (x, y, w, h).
    pub bounds: [f32; 4],
    /// Shadow color (RGBA).
    pub color: [f32; 4],
    /// Shadow offset (dx, dy).
    pub offset: [f32; 2],
    /// Gaussian blur radius.
    pub blur_radius: f32,
    /// Spread distance.
    pub spread: f32,
}

/// Parameters for a Gaussian blur pass.
#[derive(Clone, Debug)]
pub struct BlurPass {
    /// Area to blur (x, y, w, h).
    pub bounds: [f32; 4],
    /// Blur strength (standard deviation).
    pub sigma: f32,
    /// Number of downsample/upsample passes.
    pub passes: u32,
}

// ---------------------------------------------------------------------------
// EffectBatcher
// ---------------------------------------------------------------------------

/// Accumulates gradient, shadow, and blur effect instances for batched GPU
/// submission.
#[derive(Clone, Debug, Default)]
pub struct EffectBatcher {
    linear_gradients: Vec<LinearGradientInstance>,
    radial_gradients: Vec<RadialGradientInstance>,
    shadows: Vec<ShadowInstance>,
    blur_passes: Vec<BlurPass>,
}

impl EffectBatcher {
    /// Creates a new, empty [`EffectBatcher`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueues a linear gradient instance.
    pub fn add_linear_gradient(&mut self, instance: LinearGradientInstance) {
        self.linear_gradients.push(instance);
    }

    /// Enqueues a radial gradient instance.
    pub fn add_radial_gradient(&mut self, instance: RadialGradientInstance) {
        self.radial_gradients.push(instance);
    }

    /// Enqueues a shadow instance.
    pub fn add_shadow(&mut self, instance: ShadowInstance) {
        self.shadows.push(instance);
    }

    /// Enqueues a blur pass.
    pub fn add_blur(&mut self, pass: BlurPass) {
        self.blur_passes.push(pass);
    }

    /// Returns the number of queued linear gradient instances.
    pub fn linear_gradient_count(&self) -> usize {
        self.linear_gradients.len()
    }

    /// Returns the number of queued radial gradient instances.
    pub fn radial_gradient_count(&self) -> usize {
        self.radial_gradients.len()
    }

    /// Returns the number of queued shadow instances.
    pub fn shadow_count(&self) -> usize {
        self.shadows.len()
    }

    /// Read-only access to the accumulated shadow instances.
    pub fn shadows(&self) -> &[ShadowInstance] {
        &self.shadows
    }

    /// Read-only access to the accumulated linear gradient instances.
    pub fn linear_gradients(&self) -> &[LinearGradientInstance] {
        &self.linear_gradients
    }

    /// Read-only access to the accumulated radial gradient instances.
    pub fn radial_gradients(&self) -> &[RadialGradientInstance] {
        &self.radial_gradients
    }

    /// Returns the number of queued blur passes.
    pub fn blur_count(&self) -> usize {
        self.blur_passes.len()
    }

    /// Returns `true` if there are no queued effects of any kind.
    pub fn is_empty(&self) -> bool {
        self.linear_gradients.is_empty()
            && self.radial_gradients.is_empty()
            && self.shadows.is_empty()
            && self.blur_passes.is_empty()
    }

    /// Removes all queued effects, resetting all internal buffers.
    pub fn clear(&mut self) {
        self.linear_gradients.clear();
        self.radial_gradients.clear();
        self.shadows.clear();
        self.blur_passes.clear();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- helpers -------------------------------------------------------------

    fn sample_linear_gradient() -> LinearGradientInstance {
        LinearGradientInstance {
            bounds: [0.0, 0.0, 100.0, 50.0],
            start: [0.0, 0.0],
            end: [100.0, 0.0],
            stops: vec![
                GradientStop {
                    color: [1.0, 0.0, 0.0, 1.0],
                    position: 0.0,
                    _padding: [0.0; 3],
                },
                GradientStop {
                    color: [0.0, 0.0, 1.0, 1.0],
                    position: 1.0,
                    _padding: [0.0; 3],
                },
            ],
            corner_radii: [0.0; 4],
            transform: [1.0, 0.0, 0.0, 1.0],
        }
    }

    fn sample_radial_gradient() -> RadialGradientInstance {
        RadialGradientInstance {
            bounds: [10.0, 10.0, 80.0, 80.0],
            center: [50.0, 50.0],
            radius: 40.0,
            stops: vec![GradientStop {
                color: [1.0, 1.0, 1.0, 1.0],
                position: 0.0,
                _padding: [0.0; 3],
            }],
            corner_radii: [0.0; 4],
            transform: [1.0, 0.0, 0.0, 1.0],
        }
    }

    fn sample_shadow() -> ShadowInstance {
        ShadowInstance {
            bounds: [0.0, 0.0, 200.0, 100.0],
            color: [0.0, 0.0, 0.0, 0.5],
            offset: [2.0, 2.0],
            blur_radius: 4.0,
            spread: 0.0,
        }
    }

    fn sample_blur() -> BlurPass {
        BlurPass {
            bounds: [0.0, 0.0, 320.0, 240.0],
            sigma: 8.0,
            passes: 3,
        }
    }

    // -- tests ---------------------------------------------------------------

    #[test]
    fn empty_effect_batcher() {
        let batcher = EffectBatcher::new();

        assert!(batcher.is_empty());
        assert_eq!(batcher.linear_gradient_count(), 0);
        assert_eq!(batcher.radial_gradient_count(), 0);
        assert_eq!(batcher.shadow_count(), 0);
        assert_eq!(batcher.blur_count(), 0);
    }

    #[test]
    fn add_linear_gradient() {
        let mut batcher = EffectBatcher::new();
        batcher.add_linear_gradient(sample_linear_gradient());

        assert!(!batcher.is_empty());
        assert_eq!(batcher.linear_gradient_count(), 1);
    }

    #[test]
    fn add_shadow() {
        let mut batcher = EffectBatcher::new();
        batcher.add_shadow(sample_shadow());

        assert!(!batcher.is_empty());
        assert_eq!(batcher.shadow_count(), 1);
    }

    #[test]
    fn add_blur() {
        let mut batcher = EffectBatcher::new();
        batcher.add_blur(sample_blur());

        assert!(!batcher.is_empty());
        assert_eq!(batcher.blur_count(), 1);
    }

    #[test]
    fn clear_resets_all() {
        let mut batcher = EffectBatcher::new();
        batcher.add_linear_gradient(sample_linear_gradient());
        batcher.add_radial_gradient(sample_radial_gradient());
        batcher.add_shadow(sample_shadow());
        batcher.add_blur(sample_blur());

        assert!(!batcher.is_empty());

        batcher.clear();

        assert!(batcher.is_empty());
        assert_eq!(batcher.linear_gradient_count(), 0);
        assert_eq!(batcher.radial_gradient_count(), 0);
        assert_eq!(batcher.shadow_count(), 0);
        assert_eq!(batcher.blur_count(), 0);
    }

    #[test]
    fn multiple_effects_accumulate() {
        let mut batcher = EffectBatcher::new();

        batcher.add_linear_gradient(sample_linear_gradient());
        batcher.add_linear_gradient(sample_linear_gradient());
        batcher.add_radial_gradient(sample_radial_gradient());
        batcher.add_shadow(sample_shadow());
        batcher.add_shadow(sample_shadow());
        batcher.add_shadow(sample_shadow());

        assert_eq!(batcher.linear_gradient_count(), 2);
        assert_eq!(batcher.radial_gradient_count(), 1);
        assert_eq!(batcher.shadow_count(), 3);
        assert_eq!(batcher.blur_count(), 0);
        assert!(!batcher.is_empty());
    }
}
