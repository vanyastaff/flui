//! A human-readable summary of a painted frame.
//!
//! [`FrameReport`] is what [`crate::testing::FrameRun::report`] /
//! [`crate::testing::FrameRun::pump`] return. It is pure data plus a
//! [`Display`](std::fmt::Display) impl so callers (e.g. the
//! `render_inspector` example) can print it; the harness itself never
//! writes to stdout.

use std::fmt;

use flui_types::Rect;

/// A snapshot of the observable output of one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameReport {
    /// Whether the frame produced a layer tree (a clean idle frame produces
    /// none).
    pub painted: bool,
    /// The composited layer kinds in pre-order, each with its depth from the
    /// root.
    pub structure: Vec<(usize, &'static str)>,
    /// The bounds of the first picture layer, if any.
    pub picture_bounds: Option<Rect>,
    /// Whether the pipeline still has dirty nodes after the frame (a healthy
    /// settled frame leaves none).
    pub dirty: bool,
}

impl fmt::Display for FrameReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "painted: {}", self.painted)?;
        writeln!(f, "dirty:   {}", self.dirty)?;
        match self.picture_bounds {
            Some(bounds) => writeln!(f, "picture: {bounds:?}")?,
            None => writeln!(f, "picture: <none>")?,
        }
        if self.structure.is_empty() {
            write!(f, "layers:  <none>")?;
        } else {
            writeln!(f, "layers:")?;
            for (i, (depth, kind)) in self.structure.iter().enumerate() {
                let indent = "  ".repeat(*depth);
                let newline = if i + 1 == self.structure.len() {
                    ""
                } else {
                    "\n"
                };
                write!(f, "  {indent}{kind}{newline}")?;
            }
        }
        Ok(())
    }
}
