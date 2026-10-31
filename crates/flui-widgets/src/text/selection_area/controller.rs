//! [`SelectionController`]: reads and moves a [`SelectionArea`]'s selection
//! from outside the area, as an app or a driver does.
//!
//! [`SelectionArea`]: super::SelectionArea

use std::cell::RefCell;
use std::fmt;
use std::rc::{Rc, Weak};

use flui_foundation::ElementId;
use flui_foundation::panic::FirstPanic;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId, ListenerObserver};
use flui_painting::text_boundaries;
use flui_rendering::semantics::{DocumentPosition, DocumentSelection, SemanticsAnchor};
use flui_view::EventContextError;

use super::input::CaretMemory;
use super::{SelectedText, SelectionChangeCause, SelectionModel};

/// The state every clone of a [`SelectionController`] shares.
struct Inner {
    /// The area the controller is attached to, if any.
    area: RefCell<Weak<SelectionModel>>,
    notifier: ChangeNotifier,
}

impl Listenable for Inner {
    fn add_observer(&self, observer: ListenerObserver) -> ListenerId {
        self.notifier.add_observer(observer)
    }

    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(listener)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}

/// Why a [`SelectionController`] refused a selection.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SelectError {
    /// The controller is not attached to a mounted [`SelectionArea`](super::SelectionArea).
    #[error("the controller is not attached to a mounted SelectionArea")]
    NotAttached,
    /// An end names a text the area does not present: not beneath it, not
    /// laid out in it, or left out of what it reads.
    #[error("the area presents no text {0:?}")]
    UnknownText(SemanticsAnchor),
    /// An end's byte is past its text's end or splits a user-perceived
    /// character.
    #[error("byte {byte} of text {text:?} is not a grapheme boundary")]
    NotABoundary {
        /// The text the end names.
        text: SemanticsAnchor,
        /// The byte that is not a boundary of it.
        byte: usize,
    },
    /// The call came while an element was building: a selection changes in
    /// an event or after a frame, never from `build` (ADR-0086).
    #[error("the selection was changed during the build of {element:?}")]
    DuringBuild {
        /// The element whose build is in progress.
        element: ElementId,
    },
}

/// A shared handle that reads and moves the selection of the
/// [`SelectionArea`](super::SelectionArea) it is given to with
/// [`SelectionArea::controller`](super::SelectionArea::controller).
///
/// The selection is held as two ends, each a text's [`SemanticsAnchor`] and
/// a byte in that text, the same [`DocumentSelection`] assistive technology
/// moves (ADR-0214); [`texts`](Self::texts) lists the anchors in reading
/// order. A selection made here is reported to the area's
/// [`on_selection_changed`](super::SelectionArea::on_selection_changed) with
/// [`SelectionChangeCause::Programmatic`]; it does not take the focus and
/// hides the touch handles. [`listenable`](Self::listenable) notifies after
/// every change of the area's selection, whatever made it.
///
/// Every clone shares one attachment and one notification channel on the
/// owning UI thread. An unattached controller reports no selection and
/// refuses changes with [`SelectError::NotAttached`]; attaching to an area
/// or leaving it is not a change it notifies of.
#[derive(Clone)]
pub struct SelectionController {
    inner: Rc<Inner>,
}

impl fmt::Debug for SelectionController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectionController")
            .field("attached", &self.area().is_some())
            .field("selection", &self.selection())
            .finish_non_exhaustive()
    }
}

impl Default for SelectionController {
    fn default() -> Self {
        Self::new()
    }
}

impl SelectionController {
    /// A controller attached to no area.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(Inner {
                area: RefCell::new(Weak::new()),
                notifier: ChangeNotifier::new(),
            }),
        }
    }

    /// The area's selection, `None` when it selects nothing or the
    /// controller is not attached.
    #[must_use]
    pub fn selection(&self) -> Option<DocumentSelection> {
        self.area()?.selection.borrow().clone()
    }

    /// The text the area holds selected, as its listener hears it; `None`
    /// when it covers none or the controller is not attached.
    #[must_use]
    pub fn selected_text(&self) -> Option<SelectedText> {
        let text = self.area()?.selected_text()?;
        Some(SelectedText { text })
    }

    /// The texts the area presents, in the order it reads them; empty when
    /// the controller is not attached.
    #[must_use]
    pub fn texts(&self) -> Vec<SemanticsAnchor> {
        self.area().map_or_else(Vec::new, |area| {
            area.ordered()
                .into_iter()
                .map(|text| text.entry.anchor.clone())
                .collect()
        })
    }

    /// Selects `selection` in the area and tells its listener, then this
    /// controller's.
    ///
    /// # Errors
    ///
    /// The selection is left as it was when the controller is not attached,
    /// when an end names a text [`texts`](Self::texts) does not list, or
    /// when an end's byte is not a grapheme boundary of its text, and when
    /// called during a build ([`SelectError::DuringBuild`]).
    pub fn select(&self, selection: DocumentSelection) -> Result<(), SelectError> {
        let area = self.area().ok_or(SelectError::NotAttached)?;
        let ordered = area.ordered();
        for end in [selection.anchor(), selection.focus()] {
            let text = ordered
                .iter()
                .find(|text| text.entry.anchor == *end.text())
                .ok_or_else(|| SelectError::UnknownText(end.text().clone()))?;
            if text_boundaries::grapheme_floor(&text.text, end.byte()) != end.byte() {
                return Err(SelectError::NotABoundary {
                    text: end.text().clone(),
                    byte: end.byte(),
                });
            }
        }
        area.write_programmatic(Some(&selection))
    }

    /// Selects every text the area presents, whole, as far as each is laid
    /// out; an area with no text keeps its selection.
    ///
    /// # Errors
    ///
    /// [`SelectError::NotAttached`] when the controller is not attached,
    /// [`SelectError::DuringBuild`] when called during a build.
    pub fn select_all(&self) -> Result<(), SelectError> {
        let area = self.area().ok_or(SelectError::NotAttached)?;
        let ordered = area.ordered();
        let (Some(first), Some(last)) = (ordered.first(), ordered.last()) else {
            return Ok(());
        };
        let selection = DocumentSelection::new(
            DocumentPosition::new(first.entry.anchor.clone(), 0),
            DocumentPosition::new(last.entry.anchor.clone(), last.shown),
        );
        area.write_programmatic(Some(&selection))
    }

    /// Selects nothing.
    ///
    /// # Errors
    ///
    /// [`SelectError::NotAttached`] when the controller is not attached,
    /// [`SelectError::DuringBuild`] when called during a build.
    pub fn clear(&self) -> Result<(), SelectError> {
        let area = self.area().ok_or(SelectError::NotAttached)?;
        area.write_programmatic(None)
    }

    /// An `Rc<dyn Listenable>` sharing this controller's notification
    /// channel, notified after each change of the attached area's
    /// selection.
    #[must_use]
    pub fn listenable(&self) -> Rc<dyn Listenable> {
        Rc::clone(&self.inner) as Rc<dyn Listenable>
    }

    /// The mounted area the controller is attached to.
    fn area(&self) -> Option<Rc<SelectionModel>> {
        self.inner
            .area
            .borrow()
            .upgrade()
            .filter(|area| area.mounted.get())
    }

    pub(super) fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    /// Attaches the controller to `area`, which takes it from any other.
    pub(super) fn attach(&self, area: &Rc<SelectionModel>) {
        let previous = self.inner.area.replace(Rc::downgrade(area));
        debug_assert!(
            previous
                .upgrade()
                .is_none_or(|previous| Rc::ptr_eq(&previous, area) || !previous.mounted.get()),
            "BUG: one SelectionController is given to two mounted SelectionAreas"
        );
    }

    /// Detaches the controller from `area`, if attached to it.
    pub(super) fn detach(&self, area: &SelectionModel) {
        let mut attached = self.inner.area.borrow_mut();
        if std::ptr::eq(attached.as_ptr(), area) {
            *attached = Weak::new();
        }
    }

    /// Tells the controller's listeners that the selection changed. A
    /// listener that panics is held in `failure`.
    pub(super) fn notify(&self, failure: &mut FirstPanic) {
        failure.contain(|| self.inner.notifier.notify_listeners());
    }
}

impl SelectionModel {
    /// Commits `selection` for a controller, through the area's writer,
    /// without taking the focus. A call during a build is refused before
    /// anything changes.
    fn write_programmatic(&self, selection: Option<&DocumentSelection>) -> Result<(), SelectError> {
        let writer = self.writer.borrow().clone();
        let writer = writer.ok_or(SelectError::NotAttached)?;
        writer.write(|cx| {
            match writer.check_context(cx) {
                Ok(()) => {}
                Err(EventContextError::WrittenDuringBuild { element }) => {
                    return Err(SelectError::DuringBuild { element });
                }
                Err(_) => return Err(SelectError::NotAttached),
            }
            self.commit(
                cx,
                selection,
                SelectionChangeCause::Programmatic,
                CaretMemory::default(),
            );
            Ok(())
        })
    }
}
