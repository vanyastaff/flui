//! Typography values: text styles, alignment, baselines, decoration, metrics
//! and the inline span tree a `TextPainter` lays out.

mod font_settings;
mod text_alignment;
mod text_baseline;
mod text_decoration;
mod text_metrics;
mod text_spans;
mod text_style;

pub use font_settings::*;
pub use text_alignment::*;
pub use text_baseline::*;
pub use text_decoration::*;
pub use text_metrics::*;
pub use text_spans::*;
pub use text_style::*;
