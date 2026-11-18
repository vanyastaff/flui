//! The launcher entry as the application's Dock tile: the count is the
//! tile's badge, the progress a bar drawn over the application's icon in
//! the tile's content view.
//!
//! The Dock tile is the application's, not a window's, so the entry is shown
//! whether a window is open or not. The Dock draws the content view when
//! asked (`display`), not continuously, so an indeterminate bar could not
//! move: [`Progress::Indeterminate`] shows no progress, as
//! [`Honoured::indeterminate`] says. The badge shows the count in full; the
//! Dock shortens a long one itself.

use crate::taskbar::{EntryState, Honoured, Progress};

/// What the Dock shows: every part but an indeterminate bar, and it is the
/// system's own tile, so what it shows is confirmed.
pub(super) const HONOURED: Honoured = Honoured::new(true, false, true, true);

/// The tile's badge for `count`: none for no count.
pub(super) fn badge(count: Option<u64>) -> Option<String> {
    count.map(|count| count.to_string())
}

/// The bar the tile draws for `state`: how far along, from `0.0` to `1.0`,
/// or none.
pub(super) fn bar(state: EntryState) -> Option<f64> {
    match state.progress {
        Progress::Done(fraction) => Some(fraction.get()),
        Progress::Hidden | Progress::Indeterminate => None,
    }
}

#[cfg(target_os = "macos")]
pub(crate) use appkit::MacDockTile;

#[cfg(target_os = "macos")]
mod appkit {
    use objc2::{MainThreadMarker, MainThreadOnly as _};
    use objc2_app_kit::{
        NSApplication, NSImageView, NSProgressIndicator, NSProgressIndicatorStyle, NSView,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    use super::{HONOURED, badge, bar};
    use crate::taskbar::{EntryState, Honoured, TaskbarBackend};

    /// [`TaskbarBackend`] over the application's Dock tile.
    pub(crate) struct MacDockTile {
        mtm: MainThreadMarker,
    }

    impl MacDockTile {
        pub(crate) fn new(mtm: MainThreadMarker) -> Self {
            Self { mtm }
        }

        /// The tile's content view for a bar `done` along: the
        /// application's icon with the bar along its bottom.
        fn content(&self, size: NSSize, done: f64) -> objc2::rc::Retained<NSView> {
            let application = NSApplication::sharedApplication(self.mtm);
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), size);
            let view: objc2::rc::Retained<NSView> = match application.applicationIconImage() {
                Some(icon) => objc2::rc::Retained::into_super(objc2::rc::Retained::into_super(
                    NSImageView::imageViewWithImage(&icon, self.mtm),
                )),
                None => NSView::initWithFrame(NSView::alloc(self.mtm), frame),
            };
            view.setFrame(frame);
            let inset = size.width / 16.0;
            let height = size.height / 8.0;
            let bar_frame = NSRect::new(
                NSPoint::new(inset, inset),
                NSSize::new(size.width - 2.0 * inset, height),
            );
            let indicator =
                NSProgressIndicator::initWithFrame(NSProgressIndicator::alloc(self.mtm), bar_frame);
            indicator.setStyle(NSProgressIndicatorStyle::Bar);
            indicator.setIndeterminate(false);
            indicator.setMinValue(0.0);
            indicator.setMaxValue(1.0);
            indicator.setDoubleValue(done);
            view.addSubview(&indicator);
            view
        }
    }

    impl TaskbarBackend for MacDockTile {
        fn show(&self, state: EntryState) {
            let tile = NSApplication::sharedApplication(self.mtm).dockTile();
            let label = badge(state.count).map(|label| NSString::from_str(&label));
            tile.setBadgeLabel(label.as_deref());
            match bar(state) {
                Some(done) => tile.setContentView(Some(&self.content(tile.size(), done))),
                None => tile.setContentView(None),
            }
            tile.display();
        }

        fn honours(&self) -> Honoured {
            HONOURED
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HONOURED, badge, bar};
    use crate::taskbar::{EntryState, Fraction, Honoured, Progress};

    #[test]
    fn the_tile_shows_the_count_and_a_known_extent_and_says_so() {
        assert_eq!(badge(None), None);
        assert_eq!(badge(Some(0)).as_deref(), Some("0"));
        assert_eq!(badge(Some(1234)).as_deref(), Some("1234"));
        let half = Fraction::try_new(0.5).expect("a fraction");
        assert_eq!(bar(EntryState::new(Progress::Done(half), None)), Some(0.5));
        assert_eq!(bar(EntryState::new(Progress::Hidden, Some(3))), None);
        assert_eq!(
            bar(EntryState::new(Progress::Indeterminate, None)),
            None,
            "a tile drawn once cannot move a bar"
        );
        assert_eq!(
            HONOURED,
            Honoured::new(true, false, true, true),
            "the Dock shows a count and a known extent, and it is the system's own tile"
        );
    }
}
