//! The outputs a surface is on, as its window reads them from the
//! display snapshot: the first it entered names the window's display,
//! and the densest sets the scale its buffer is drawn at where the
//! compositor prefers none.

#![forbid(unsafe_code)]

use flui_foundation::geometry::DevicePixelRatio;
use flui_platform::DisplayId;

/// What the display snapshot says of one output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OutputFacts {
    /// The display it is.
    pub(crate) display: DisplayId,
    /// Its device pixels per logical pixel.
    pub(crate) scale: DevicePixelRatio,
}

/// The outputs a surface is on, read from one snapshot.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct OnOutputs {
    /// What the snapshot says of the first output the surface entered
    /// and has not left: the window's display, which stays there as the
    /// surface spreads to another; `None` while the snapshot does not
    /// hold that output yet, even where it holds a later one, so the
    /// display does not pass through the later one on its way.
    pub(crate) first: Option<OutputFacts>,
    /// The largest scale among the outputs the snapshot holds: a window
    /// across a scale-1 and a scale-2 output is drawn for the denser
    /// one, which the compositor scales down on the other, rather than
    /// at 1 and blurred where it is denser. A window's scale is its own,
    /// not its display's.
    pub(crate) densest: Option<DevicePixelRatio>,
}

impl OnOutputs {
    /// The outputs of `entered`, in the order the surface entered them:
    /// the ones still `live`, each as `facts` reads it from the
    /// snapshot.
    pub(crate) fn read<O: Copy>(
        entered: &[O],
        live: impl Fn(O) -> bool,
        facts: impl Fn(O) -> Option<OutputFacts>,
    ) -> Self {
        let mut on = entered
            .iter()
            .copied()
            .filter(|output| live(*output))
            .map(|output| (output, facts(output)));
        let Some((_, first)) = on.next() else {
            return Self::default();
        };
        let densest = std::iter::once(first)
            .chain(on.map(|(_, facts)| facts))
            .flatten()
            .map(|facts| facts.scale)
            .reduce(|densest, scale| {
                if scale.get() > densest.get() {
                    scale
                } else {
                    densest
                }
            });
        Self { first, densest }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::{DisplayKey, DisplayRegistry};

    use super::*;

    /// A window is on the display of the first live output it entered,
    /// as the snapshot holds it, and drawn for the densest the snapshot
    /// holds, from whichever side it entered. An output gone, or one the
    /// snapshot does not hold yet, has no say in the scale; one not held
    /// yet that the surface entered first leaves the window on no
    /// display rather than on the next.
    #[test]
    fn a_window_is_on_its_first_output_and_drawn_for_the_densest() {
        let registry = DisplayRegistry::new();
        let display = || registry.mint(&DisplayKey::Unkeyed).expect("an id");
        let ratio = |scale| DevicePixelRatio::try_new(scale).expect("a ratio");
        // Outputs 1 to 3 at scales 1, 2 and 1.5; 4 is not in the
        // snapshot yet; 5 is gone.
        let held: Vec<(u32, OutputFacts)> = [(1, 1.0), (2, 2.0), (3, 1.5), (5, 3.0)]
            .into_iter()
            .map(|(name, scale)| {
                let facts = OutputFacts {
                    display: display(),
                    scale: ratio(scale),
                };
                (name, facts)
            })
            .collect();
        let facts = |name: u32| {
            held.iter()
                .find(|(held, _)| *held == name)
                .map(|(_, facts)| *facts)
        };
        let live = |name: u32| name != 5;
        let display_of = |name: u32| facts(name).map(|facts| facts.display);
        for (case, entered, first, densest) in [
            ("on none", &[][..], None, None),
            ("on one", &[2][..], display_of(2), Some(2.0)),
            (
                "entered from the sparser",
                &[1, 2][..],
                display_of(1),
                Some(2.0),
            ),
            (
                "entered from the denser",
                &[2, 1][..],
                display_of(2),
                Some(2.0),
            ),
            ("on three", &[1, 3, 2][..], display_of(1), Some(2.0)),
            ("a gone output first", &[5, 1][..], display_of(1), Some(1.0)),
            (
                "a gone denser output",
                &[1, 5][..],
                display_of(1),
                Some(1.0),
            ),
            ("first not held yet", &[4, 2][..], None, Some(2.0)),
        ] {
            let on = OnOutputs::read(entered, live, facts);
            assert_eq!(on.first.map(|facts| facts.display), first, "{case}");
            assert_eq!(on.densest.map(DevicePixelRatio::get), densest, "{case}");
        }
    }
}
