//! [`AnimatedIconData`]: an icon drawn as filled vector paths whose points and opacities move
//! through key frames.
//!
//! Constant data ([`IconPath`], [`IconCommand`]) borrows `'static` slices and has no
//! destructor, so it can be a `const`; data built at run time ([`IconPathBuf`],
//! [`IconCommandBuf`]) owns its frames and is shared once checked. Both clone in constant time
//! inside an [`AnimatedIconData`].

use std::sync::Arc;

use flui_animation::{Cubic, Curve};
use flui_foundation::geometry::Point;
use flui_painting::paint::Path;

/// One command's frames, borrowed from either kind of data.
#[derive(Debug, Clone, Copy)]
enum CommandRef<'a> {
    MoveTo(&'a [[f64; 2]]),
    LineTo(&'a [[f64; 2]]),
    /// Control point, control point, end point: `[x1, y1, x2, y2, x, y]`.
    CubicTo(&'a [[f64; 6]]),
    Close,
}

impl CommandRef<'_> {
    fn frames(self) -> Option<usize> {
        match self {
            Self::MoveTo(points) | Self::LineTo(points) => Some(points.len()),
            Self::CubicTo(points) => Some(points.len()),
            Self::Close => None,
        }
    }

    fn all_finite(self) -> bool {
        match self {
            Self::MoveTo(points) | Self::LineTo(points) => {
                points.iter().flatten().all(|v| v.is_finite())
            }
            Self::CubicTo(points) => points.iter().flatten().all(|v| v.is_finite()),
            Self::Close => true,
        }
    }
}

/// One drawing command of an [`IconPath`], with its points at every key frame, as constant
/// data.
///
/// Points are in the icon's view box. Every command of a path carries one entry per key
/// frame of its [`AnimatedIconData`].
#[derive(Debug, Clone, Copy)]
pub struct IconCommand(CommandRef<'static>);

impl IconCommand {
    /// Start a contour at each frame's point.
    #[must_use]
    pub const fn move_to(frames: &'static [[f64; 2]]) -> Self {
        Self(CommandRef::MoveTo(frames))
    }

    /// A line to each frame's point.
    #[must_use]
    pub const fn line_to(frames: &'static [[f64; 2]]) -> Self {
        Self(CommandRef::LineTo(frames))
    }

    /// A cubic Bézier to each frame's `[x1, y1, x2, y2, x, y]`.
    #[must_use]
    pub const fn cubic_to(frames: &'static [[f64; 6]]) -> Self {
        Self(CommandRef::CubicTo(frames))
    }

    /// Close the contour.
    #[must_use]
    pub const fn close() -> Self {
        Self(CommandRef::Close)
    }
}

/// [`IconCommand`] built at run time, owning its frames.
#[derive(Debug, Clone, PartialEq)]
pub struct IconCommandBuf(CommandBuf);

#[derive(Debug, Clone, PartialEq)]
enum CommandBuf {
    MoveTo(Vec<[f64; 2]>),
    LineTo(Vec<[f64; 2]>),
    CubicTo(Vec<[f64; 6]>),
    Close,
}

impl IconCommandBuf {
    /// Start a contour at each frame's point.
    #[must_use]
    pub fn move_to(frames: Vec<[f64; 2]>) -> Self {
        Self(CommandBuf::MoveTo(frames))
    }

    /// A line to each frame's point.
    #[must_use]
    pub fn line_to(frames: Vec<[f64; 2]>) -> Self {
        Self(CommandBuf::LineTo(frames))
    }

    /// A cubic Bézier to each frame's `[x1, y1, x2, y2, x, y]`.
    #[must_use]
    pub fn cubic_to(frames: Vec<[f64; 6]>) -> Self {
        Self(CommandBuf::CubicTo(frames))
    }

    /// Close the contour.
    #[must_use]
    pub fn close() -> Self {
        Self(CommandBuf::Close)
    }

    fn view(&self) -> CommandRef<'_> {
        match &self.0 {
            CommandBuf::MoveTo(points) => CommandRef::MoveTo(points),
            CommandBuf::LineTo(points) => CommandRef::LineTo(points),
            CommandBuf::CubicTo(points) => CommandRef::CubicTo(points),
            CommandBuf::Close => CommandRef::Close,
        }
    }
}

/// One filled path of an [`AnimatedIconData`], as constant data: its opacity and its
/// commands at every frame.
#[derive(Debug, Clone, Copy)]
pub struct IconPath {
    opacities: &'static [f64],
    commands: &'static [IconCommand],
}

impl IconPath {
    /// A path's opacity at every frame and its commands.
    #[must_use]
    pub const fn new(opacities: &'static [f64], commands: &'static [IconCommand]) -> Self {
        Self {
            opacities,
            commands,
        }
    }
}

/// [`IconPath`] built at run time, owning its frames.
#[derive(Debug, Clone, PartialEq)]
pub struct IconPathBuf {
    opacities: Vec<f64>,
    commands: Vec<IconCommandBuf>,
}

impl IconPathBuf {
    /// A path's opacity at every frame and its commands.
    #[must_use]
    pub fn new(opacities: Vec<f64>, commands: Vec<IconCommandBuf>) -> Self {
        Self {
            opacities,
            commands,
        }
    }
}

/// Why [`AnimatedIconData::try_new`] or [`AnimatedIconData::check`] refused icon data.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IconDataError {
    /// There are no key frames, or no paths.
    #[error("animated icon data has no key frames or no paths")]
    Empty,
    /// The frame times do not start at exactly 0, end at exactly 1 and rise strictly.
    #[error("animated icon frame times must rise strictly from exactly 0 to exactly 1")]
    Times,
    /// The view box is not finite and positive.
    #[error("the animated icon view box must be finite and positive")]
    ViewBox,
    /// A path or command has a different number of frames from the times.
    #[error("path {path} does not have one entry per key frame")]
    FrameCount {
        /// The path's index.
        path: usize,
    },
    /// A coordinate or an opacity is not finite.
    #[error("path {path} has a value that is not finite")]
    NonFinite {
        /// The path's index.
        path: usize,
    },
    /// An opacity is outside `[0, 1]`.
    #[error("path {path} has an opacity outside [0, 1]")]
    Opacity {
        /// The path's index.
        path: usize,
    },
    /// The curves between frames are neither none nor one per gap between frames.
    #[error("animated icon curves must number one per gap between frames")]
    Curves,
    /// Interpolating between two frames leaves the finite range.
    #[error("path {path} overflows between key frames")]
    Overflow {
        /// The path's index.
        path: usize,
    },
}

/// The frames, curves and paths, constant or shared.
#[derive(Debug, Clone)]
enum Body {
    Static {
        times: &'static [f64],
        curves: &'static [Cubic],
        paths: &'static [IconPath],
    },
    Shared(Arc<Owned>),
}

#[derive(Debug)]
struct Owned {
    times: Vec<f64>,
    curves: Vec<Cubic>,
    paths: Vec<IconPathBuf>,
}

/// An icon drawn as filled vector paths that move through key frames, for
/// [`AnimatedIcon`](super::AnimatedIcon).
///
/// Frame times rise strictly from exactly 0 to exactly 1, so any progress in `[0, 1]` falls
/// between two frames; a still icon has two identical frames. Every path carries one opacity
/// per frame, and each of its commands one point set per frame, so every frame's path has the
/// same commands and the points move from frame to frame. Between two frames the points and
/// the opacity move along the gap's curve, linearly without curves.
///
/// Constant data (the Material pairs in `flui-material`) is built with
/// [`from_static`](Self::from_static) and checked by a table test with
/// [`check`](Self::check); data built at run time goes through [`try_new`](Self::try_new).
/// `cargo xtask icon-import` turns an Android AnimatedVectorDrawable or SVG key frames into
/// constant data.
#[derive(Debug, Clone)]
pub struct AnimatedIconData {
    view_box: f64,
    body: Body,
    match_text_direction: bool,
}

impl AnimatedIconData {
    /// Constant data: a square view box `view_box` units across, the frame times, the curve
    /// of each gap between frames (empty for linear), the paths, and whether the icon mirrors
    /// under right-to-left text.
    ///
    /// The caller vouches for the data; a table test checks constant data with
    /// [`check`](Self::check).
    #[must_use]
    pub const fn from_static(
        view_box: f64,
        times: &'static [f64],
        curves: &'static [Cubic],
        paths: &'static [IconPath],
        match_text_direction: bool,
    ) -> Self {
        Self {
            view_box,
            body: Body::Static {
                times,
                curves,
                paths,
            },
            match_text_direction,
        }
    }

    /// Checked run-time data; see [`from_static`](Self::from_static) for the parts.
    ///
    /// # Errors
    ///
    /// [`IconDataError`] when there are no frames or paths, the times do not rise strictly
    /// from 0 to 1, the view box is not finite and positive, a path or command lacks one entry
    /// per frame, a value is not finite, an opacity is outside `[0, 1]`, the curves are not
    /// one per gap, or a frame between two key frames would overflow.
    pub fn try_new(
        view_box: f64,
        times: Vec<f64>,
        curves: Vec<Cubic>,
        paths: Vec<IconPathBuf>,
        match_text_direction: bool,
    ) -> Result<Self, IconDataError> {
        let data = Self {
            view_box,
            body: Body::Shared(Arc::new(Owned {
                times,
                curves,
                paths,
            })),
            match_text_direction,
        };
        data.check()?;
        Ok(data)
    }

    fn times(&self) -> &[f64] {
        match &self.body {
            Body::Static { times, .. } => times,
            Body::Shared(owned) => &owned.times,
        }
    }

    fn curves(&self) -> &[Cubic] {
        match &self.body {
            Body::Static { curves, .. } => curves,
            Body::Shared(owned) => &owned.curves,
        }
    }

    /// Path `index`'s opacities and commands.
    fn path(&self, index: usize) -> (&[f64], Vec<CommandRef<'_>>) {
        match &self.body {
            Body::Static { paths, .. } => {
                let path = &paths[index];
                (path.opacities, path.commands.iter().map(|c| c.0).collect())
            }
            Body::Shared(owned) => {
                let path = &owned.paths[index];
                (
                    &path.opacities,
                    path.commands.iter().map(IconCommandBuf::view).collect(),
                )
            }
        }
    }

    /// How many paths the icon draws.
    #[must_use]
    pub fn path_count(&self) -> usize {
        match &self.body {
            Body::Static { paths, .. } => paths.len(),
            Body::Shared(owned) => owned.paths.len(),
        }
    }

    /// How many key frames the icon has.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.times().len()
    }

    /// The view box's side, in the units of the paths' points.
    #[must_use]
    pub fn view_box(&self) -> f64 {
        self.view_box
    }

    /// Whether the icon mirrors under right-to-left text.
    #[must_use]
    pub fn match_text_direction(&self) -> bool {
        self.match_text_direction
    }

    /// Checks the data against every rule of [`try_new`](Self::try_new).
    ///
    /// # Errors
    ///
    /// The first rule the data breaks.
    pub fn check(&self) -> Result<(), IconDataError> {
        if !self.view_box.is_finite() || self.view_box <= 0.0 {
            return Err(IconDataError::ViewBox);
        }
        let times = self.times();
        if times.is_empty() || self.path_count() == 0 {
            return Err(IconDataError::Empty);
        }
        let rising = times.windows(2).all(|pair| pair[0] < pair[1]);
        if times.first() != Some(&0.0) || times.last() != Some(&1.0) || !rising {
            return Err(IconDataError::Times);
        }
        let curves = self.curves().len();
        if curves != 0 && curves != times.len() - 1 {
            return Err(IconDataError::Curves);
        }
        for index in 0..self.path_count() {
            let (opacities, commands) = self.path(index);
            if opacities.len() != times.len()
                || commands
                    .iter()
                    .any(|command| command.frames().is_some_and(|n| n != times.len()))
            {
                return Err(IconDataError::FrameCount { path: index });
            }
            if opacities.iter().any(|o| !o.is_finite()) || !commands.iter().all(|c| c.all_finite())
            {
                return Err(IconDataError::NonFinite { path: index });
            }
            if opacities.iter().any(|o| !(0.0..=1.0).contains(o)) {
                return Err(IconDataError::Opacity { path: index });
            }
            // A curve may overshoot its gap; the overshoot must stay finite. Each gap's ends
            // and its curve's middle are checked.
            for frame in 0..times.len() - 1 {
                for share in [0.0, 0.5, 1.0] {
                    if self
                        .path_between(index, frame, self.ease(frame, share))
                        .is_none()
                    {
                        return Err(IconDataError::Overflow { path: index });
                    }
                }
            }
        }
        Ok(())
    }

    /// The share of the gap after `frame` that `share` of its time reaches, through the gap's
    /// curve.
    fn ease(&self, frame: usize, share: f64) -> f64 {
        match self.curves().get(frame) {
            Some(curve) => curve.transform(share),
            None => share,
        }
    }

    /// The gap that `progress` falls in, and the eased share of it reached.
    fn locate(&self, progress: f64) -> (usize, f64) {
        let times = self.times();
        let last = times.len().saturating_sub(1);
        if last == 0 {
            return (0, 0.0);
        }
        let progress = progress.clamp(0.0, 1.0);
        // The gap starts at the last frame at or before `progress`.
        let frame = times[1..]
            .iter()
            .position(|&time| time > progress)
            .unwrap_or(last - 1)
            .min(last - 1);
        let (from, to) = (times[frame], times[frame + 1]);
        let share = ((progress - from) / (to - from)).clamp(0.0, 1.0);
        (frame, self.ease(frame, share))
    }

    /// Every path at `progress`, with its opacity: `None` when any of them is not finite.
    pub(super) fn frame(&self, progress: f64) -> Option<Vec<(Path, f64)>> {
        let (frame, share) = self.locate(progress);
        (0..self.path_count())
            .map(|index| self.path_between(index, frame, share))
            .collect()
    }

    /// Path `index` `share` of the way from key frame `frame` to the next one.
    fn path_between(&self, index: usize, frame: usize, share: f64) -> Option<(Path, f64)> {
        let next = (frame + 1).min(self.times().len().saturating_sub(1));
        // Weighted ends, not `a + (b - a) * share`: the difference of two finite extremes
        // overflows where the weighted sum stays between them.
        let lerp = |a: f64, b: f64| a * (1.0 - share) + b * share;
        let (opacities, commands) = self.path(index);
        let mut out = Path::new();
        let mut finite = true;
        let mut point = |x: f64, y: f64| {
            finite &= x.is_finite() && y.is_finite();
            Point::new(x, y)
        };
        for command in commands {
            match command {
                CommandRef::MoveTo(points) => {
                    let (a, b) = (points[frame], points[next]);
                    out.move_to(point(lerp(a[0], b[0]), lerp(a[1], b[1])));
                }
                CommandRef::LineTo(points) => {
                    let (a, b) = (points[frame], points[next]);
                    out.line_to(point(lerp(a[0], b[0]), lerp(a[1], b[1])));
                }
                CommandRef::CubicTo(points) => {
                    let (a, b) = (points[frame], points[next]);
                    let c1 = point(lerp(a[0], b[0]), lerp(a[1], b[1]));
                    let c2 = point(lerp(a[2], b[2]), lerp(a[3], b[3]));
                    let end = point(lerp(a[4], b[4]), lerp(a[5], b[5]));
                    out.cubic_to(c1, c2, end);
                }
                CommandRef::Close => out.close(),
            }
        }
        let opacity = lerp(opacities[frame], opacities[next]).clamp(0.0, 1.0);
        (finite && opacity.is_finite()).then_some((out, opacity))
    }
}
