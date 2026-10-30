//! What a [`SelectionArea`](super::SelectionArea) has selected, and the controller that reads and
//! sets it without a pointer.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use flui_foundation::notifier::{ChangeNotifier, Listenable};

use super::area::AreaCore;
use crate::text::SelectableId;

/// A position in one paragraph of a [`SelectionArea`](super::SelectionArea): a UTF-8 byte offset
/// into that paragraph's plain text, on a character boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SelectionPoint {
    /// The paragraph, as the area minted its id.
    pub paragraph: SelectableId,
    /// The byte offset in the paragraph's plain text.
    pub offset: usize,
}

impl SelectionPoint {
    /// The point at byte `offset` of `paragraph`.
    #[must_use]
    pub const fn new(paragraph: SelectableId, offset: usize) -> Self {
        Self { paragraph, offset }
    }
}

/// An area's selection: where it started and where it is moving. `focus` may come before
/// `anchor` in reading order; the selected text lies between them either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AreaSelection {
    /// The end that stays when the selection is extended.
    pub anchor: SelectionPoint,
    /// The end that moves when the selection is extended.
    pub focus: SelectionPoint,
}

impl AreaSelection {
    /// The selection from `anchor` to `focus`.
    #[must_use]
    pub const fn new(anchor: SelectionPoint, focus: SelectionPoint) -> Self {
        Self { anchor, focus }
    }
}

/// What an area has selected, in reading order.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SelectedContent {
    /// The selected text, one line per paragraph it spans.
    pub plain_text: String,
}

/// Why [`SelectionController::select`] refused a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SelectionError {
    /// The controller is not attached to a mounted area, or the area is not laid out yet.
    #[error("the controller is not attached to a laid-out SelectionArea")]
    NotAttached,
    /// A point names a paragraph this area does not hold: unmounted, or another area's.
    #[error("{0:?} is not a paragraph of this SelectionArea")]
    UnknownParagraph(SelectableId),
    /// A point's offset is past its paragraph's text or inside a character.
    #[error("offset {offset} is not a character boundary of {paragraph:?}")]
    NotABoundary {
        /// The paragraph.
        paragraph: SelectableId,
        /// The refused offset.
        offset: usize,
    },
}

/// Reads and sets the selection of the [`SelectionArea`](super::SelectionArea) it is given to,
/// with no pointer or key involved: an app's "Copy all", a test, an automation driver.
///
/// Cheap to clone; every clone names the same controller. One controller serves one mounted
/// area at a time; while none holds it, it reads as nothing selected and refuses to select.
/// Changes made through it do not call the area's `on_selection_changed`, which reports only
/// what the user and assistive technology do; its [`listenable`](Self::listenable) notifies on
/// every change, whoever made it.
#[derive(Clone, Default)]
pub struct SelectionController {
    shared: Rc<ControllerShared>,
}

#[derive(Default)]
struct ControllerShared {
    area: RefCell<Weak<AreaCore>>,
    changed: Rc<ChangeNotifier>,
}

impl SelectionController {
    /// A controller attached to no area yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The current selection, or `None` when nothing is selected.
    #[must_use]
    pub fn selection(&self) -> Option<AreaSelection> {
        self.area()?.selection()
    }

    /// The selected text, or `None` when nothing is selected.
    #[must_use]
    pub fn content(&self) -> Option<SelectedContent> {
        self.area()?.content()
    }

    /// The area's paragraphs in reading order, for building [`SelectionPoint`]s.
    #[must_use]
    pub fn paragraphs(&self) -> Vec<SelectableId> {
        self.area()
            .map(|area| area.paragraphs())
            .unwrap_or_default()
    }

    /// Select everything in the area.
    pub fn select_all(&self) {
        if let Some(area) = self.area() {
            area.select_all(super::area::Origin::App);
        }
    }

    /// Select from `selection.anchor` to `selection.focus`.
    ///
    /// # Errors
    ///
    /// [`SelectionError`] when the controller has no laid-out area, or a point does not name a
    /// character boundary of one of its paragraphs. A refused selection leaves the current one.
    pub fn select(&self, selection: AreaSelection) -> Result<(), SelectionError> {
        self.area()
            .ok_or(SelectionError::NotAttached)?
            .select_checked(selection)
    }

    /// Select nothing.
    pub fn clear(&self) {
        if let Some(area) = self.area() {
            area.set(None, super::area::Origin::App);
        }
    }

    /// Notifies after every change of the selection.
    #[must_use]
    pub fn listenable(&self) -> Rc<dyn Listenable> {
        Rc::clone(&self.shared.changed) as Rc<dyn Listenable>
    }

    fn area(&self) -> Option<Rc<AreaCore>> {
        self.shared.area.borrow().upgrade()
    }

    /// Serve `area` from now on.
    pub(super) fn attach(&self, area: &Rc<AreaCore>) {
        let _previous = self.shared.area.replace(Rc::downgrade(area));
    }

    /// Stop serving `area`, if it is the one served.
    pub(super) fn detach(&self, area: &Rc<AreaCore>) {
        let served = self.shared.area.borrow().upgrade();
        if served.is_some_and(|served| Rc::ptr_eq(&served, area)) {
            let _previous = self.shared.area.replace(Weak::new());
        }
    }

    pub(super) fn notify(&self) {
        self.shared.changed.notify_listeners();
    }

    pub(super) fn same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.shared, &other.shared)
    }
}

impl std::fmt::Debug for SelectionController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionController")
            .field("attached", &self.area().is_some())
            .finish()
    }
}
