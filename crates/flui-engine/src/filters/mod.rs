//! GPU passes that read a layer's offscreen texture and write a filtered one:
//! blur, color matrix, gamma, blend mode, morphology, affine resample, the
//! image-source filter and the advanced blend composite.

pub(crate) mod advanced_blend;
pub(crate) mod blur;
pub(crate) mod color_matrix;
pub(crate) mod gamma;
pub(crate) mod image_source;
pub(crate) mod mode;
pub(crate) mod morphology;
pub(crate) mod resample;
