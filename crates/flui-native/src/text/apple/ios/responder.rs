//! The responder the system keyboard types into (ADR-0255 decision 11): an
//! invisible `UITextInput` view beside the window's `FluiView`, which never
//! becomes first responder itself. It holds the document the keyboard edits
//! ([`Composition`]) and tells the focused field what each edit means, in the
//! IME vocabulary AppKit's text input speaks: the composition as it reads,
//! the text committed, a delete before the caret.
//!
//! UIKit asks a `UITextInput` for positions and ranges as objects of its own
//! making: [`TextPosition`] and [`TextRange`] carry an offset into the
//! document, in UTF-16 units.

use std::cell::{Cell, OnceCell, RefCell};
use std::ops::Range;
use std::sync::Weak;

use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState, NamedKey};
use flui_platform::{EventTime, ImeEvent, PlatformInput};
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};

use objc2_foundation::{
    NSArray, NSAttributedStringKey, NSComparisonResult, NSDictionary, NSInteger, NSObject,
    NSObjectProtocol, NSPoint as CGPoint, NSRange, NSRect as CGRect, NSString,
};
use objc2_ui_kit::{
    NSWritingDirection, UIKeyInput, UIResponder, UITextInput, UITextInputDelegate,
    UITextInputStringTokenizer, UITextInputTokenizer, UITextInputTraits, UITextLayoutDirection,
    UITextPosition, UITextRange, UITextSelectionRect, UITextStorageDirection, UIView,
};

use super::super::composition::{Composition, Edit};
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use crate::owner::window_delivery::WindowDelivery;

/// What the target of the keyboard's log lines is.
const LOG: &str = "flui.text.ios";

define_class!(
    /// An offset into a [`TextInputView`]'s document.
    #[unsafe(super(UITextPosition, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FluiTextPosition"]
    #[ivars = usize]
    pub(super) struct TextPosition;
);

impl TextPosition {
    fn new(mtm: MainThreadMarker, offset: usize) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(offset);
        // SAFETY: NSObject's designated initializer, on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }

    fn offset(&self) -> usize {
        *self.ivars()
    }
}

/// The ends of a [`TextRange`].
pub(super) struct Ends {
    start: Retained<TextPosition>,
    end: Retained<TextPosition>,
}

define_class!(
    /// A range of a [`TextInputView`]'s document.
    #[unsafe(super(UITextRange, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FluiTextRange"]
    #[ivars = Ends]
    pub(super) struct TextRange;

    impl TextRange {
        #[unsafe(method(isEmpty))]
        fn is_empty(&self) -> bool {
            os_entry("FluiTextRange isEmpty", (), AfterPanic::Answer(|()| true), || {
                self.ivars().start.offset() == self.ivars().end.offset()
            })
        }

        #[unsafe(method_id(start))]
        fn start(&self) -> Retained<UITextPosition> {
            os_entry("FluiTextRange start", (), AfterPanic::Abort, || {
                Retained::into_super(self.ivars().start.clone())
            })
        }

        #[unsafe(method_id(end))]
        fn end(&self) -> Retained<UITextPosition> {
            os_entry("FluiTextRange end", (), AfterPanic::Abort, || {
                Retained::into_super(self.ivars().end.clone())
            })
        }
    }
);

impl TextRange {
    fn new(mtm: MainThreadMarker, range: Range<usize>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ends {
            start: TextPosition::new(mtm, range.start),
            end: TextPosition::new(mtm, range.end),
        });
        // SAFETY: NSObject's designated initializer, on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }

    fn range(&self) -> Range<usize> {
        self.ivars().start.offset()..self.ivars().end.offset()
    }
}

/// What a [`TextInputView`] keeps.
pub(super) struct ResponderIvars {
    document: RefCell<Composition>,
    /// The window the edits are told to.
    delivery: Weak<WindowDelivery>,
    /// Where the focused field's caret is, in the window's logical points,
    /// which are this view's: it sits at the window view's origin.
    caret: Cell<CGRect>,
    tokenizer: OnceCell<Retained<UITextInputStringTokenizer>>,
    input_delegate: RefCell<Option<ObjcWeak<ProtocolObject<dyn UITextInputDelegate>>>>,
}

define_class!(
    /// The invisible view the system keyboard types into.
    #[unsafe(super(UIView, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FluiTextInputView"]
    #[ivars = ResponderIvars]
    pub(super) struct TextInputView;

    impl TextInputView {
        #[unsafe(method(canBecomeFirstResponder))]
        fn can_become_first_responder(&self) -> bool {
            true
        }
    }

    unsafe impl NSObjectProtocol for TextInputView {}

    unsafe impl UITextInputTraits for TextInputView {}

    unsafe impl UIKeyInput for TextInputView {
        #[unsafe(method(hasText))]
        fn has_text(&self) -> bool {
            os_entry("FluiTextInputView hasText", (), AfterPanic::Answer(|()| false), || {
                self.ivars().document.borrow().has_text()
            })
        }

        #[unsafe(method(insertText:))]
        fn insert_text(&self, text: &NSString) {
            os_entry("FluiTextInputView insertText:", (), AfterPanic::Answer(|()| ()), || {
                let units = utf16(text);
                let edit = self.ivars().document.borrow_mut().insert(&units);
                self.announce("insertText:", Some(edit));
            });
        }

        #[unsafe(method(deleteBackward))]
        fn delete_backward(&self) {
            os_entry("FluiTextInputView deleteBackward", (), AfterPanic::Answer(|()| ()), || {
                let edit = self.ivars().document.borrow_mut().delete_backward();
                self.announce("deleteBackward", edit);
            });
        }
    }

    unsafe impl UITextInput for TextInputView {
        #[unsafe(method_id(textInRange:))]
        fn text_in_range(&self, range: &UITextRange) -> Option<Retained<NSString>> {
            os_entry("FluiTextInputView textInRange:", (), AfterPanic::Answer(|()| None), || {
                let range = ours(range)?;
                let document = self.ivars().document.borrow();
                Some(NSString::from_str(&String::from_utf16_lossy(document.text_in(range))))
            })
        }

        #[unsafe(method(replaceRange:withText:))]
        fn replace_range(&self, range: &UITextRange, text: &NSString) {
            os_entry("FluiTextInputView replaceRange:withText:", (), AfterPanic::Answer(|()| ()), || {
                let Some(range) = ours(range) else { return };
                let units = utf16(text);
                let edit = {
                    let mut document = self.ivars().document.borrow_mut();
                    document.select(range);
                    document.insert(&units)
                };
                self.announce("replaceRange:withText:", Some(edit));
            });
        }

        #[unsafe(method_id(selectedTextRange))]
        fn selected_text_range(&self) -> Option<Retained<UITextRange>> {
            os_entry("FluiTextInputView selectedTextRange", (), AfterPanic::Answer(|()| None), || {
                let selection = self.ivars().document.borrow().selection();
                Some(Retained::into_super(TextRange::new(self.mtm(), selection)))
            })
        }

        #[unsafe(method(setSelectedTextRange:))]
        fn set_selected_text_range(&self, range: Option<&UITextRange>) {
            os_entry("FluiTextInputView setSelectedTextRange:", (), AfterPanic::Answer(|()| ()), || {
                if let Some(range) = range.and_then(ours) {
                    self.ivars().document.borrow_mut().select(range);
                }
            });
        }

        #[unsafe(method_id(markedTextRange))]
        fn marked_text_range(&self) -> Option<Retained<UITextRange>> {
            os_entry("FluiTextInputView markedTextRange", (), AfterPanic::Answer(|()| None), || {
                let marked = self.ivars().document.borrow().marked()?;
                Some(Retained::into_super(TextRange::new(self.mtm(), marked)))
            })
        }

        #[unsafe(method_id(markedTextStyle))]
        fn marked_text_style(&self) -> Option<Retained<NSDictionary<NSAttributedStringKey, objc2::runtime::AnyObject>>> {
            None
        }

        #[unsafe(method(setMarkedTextStyle:))]
        fn set_marked_text_style(&self, _style: Option<&NSDictionary<NSAttributedStringKey, objc2::runtime::AnyObject>>) {}

        #[unsafe(method(setMarkedText:selectedRange:))]
        fn set_marked_text(&self, marked: Option<&NSString>, selected: NSRange) {
            os_entry("FluiTextInputView setMarkedText:selectedRange:", (), AfterPanic::Answer(|()| ()), || {
                let units = marked.map(utf16).unwrap_or_default();
                let selected = selected.location..selected.location.saturating_add(selected.length);
                let edit = self.ivars().document.borrow_mut().set_marked(&units, selected);
                self.announce("setMarkedText:selectedRange:", Some(edit));
            });
        }

        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            os_entry("FluiTextInputView unmarkText", (), AfterPanic::Answer(|()| ()), || {
                let edit = self.ivars().document.borrow_mut().unmark();
                self.announce("unmarkText", edit);
            });
        }

        #[unsafe(method_id(beginningOfDocument))]
        fn beginning_of_document(&self) -> Retained<UITextPosition> {
            os_entry("FluiTextInputView beginningOfDocument", (), AfterPanic::Abort, || {
                Retained::into_super(TextPosition::new(self.mtm(), 0))
            })
        }

        #[unsafe(method_id(endOfDocument))]
        fn end_of_document(&self) -> Retained<UITextPosition> {
            os_entry("FluiTextInputView endOfDocument", (), AfterPanic::Abort, || {
                let end = self.ivars().document.borrow().len();
                Retained::into_super(TextPosition::new(self.mtm(), end))
            })
        }

        #[unsafe(method_id(textRangeFromPosition:toPosition:))]
        fn text_range_from_position(&self, from: &UITextPosition, to: &UITextPosition) -> Option<Retained<UITextRange>> {
            os_entry("FluiTextInputView textRangeFromPosition:toPosition:", (), AfterPanic::Answer(|()| None), || {
                let range = self.ivars().document.borrow().clamp_range(offset(from)?..offset(to)?);
                Some(Retained::into_super(TextRange::new(self.mtm(), range)))
            })
        }

        #[unsafe(method_id(positionFromPosition:offset:))]
        fn position_from_position(&self, position: &UITextPosition, by: NSInteger) -> Option<Retained<UITextPosition>> {
            os_entry("FluiTextInputView positionFromPosition:offset:", (), AfterPanic::Answer(|()| None), || {
                self.moved(offset(position)?, by)
            })
        }

        #[unsafe(method_id(positionFromPosition:inDirection:offset:))]
        fn position_in_direction(&self, position: &UITextPosition, direction: UITextLayoutDirection, by: NSInteger) -> Option<Retained<UITextPosition>> {
            os_entry("FluiTextInputView positionFromPosition:inDirection:offset:", (), AfterPanic::Answer(|()| None), || {
                let by = if backward(direction) { by.checked_neg()? } else { by };
                self.moved(offset(position)?, by)
            })
        }

        #[unsafe(method(comparePosition:toPosition:))]
        fn compare_position(&self, position: &UITextPosition, other: &UITextPosition) -> NSComparisonResult {
            os_entry("FluiTextInputView comparePosition:toPosition:", (), AfterPanic::Answer(|()| NSComparisonResult::Same), || {
                match offset(position).cmp(&offset(other)) {
                    std::cmp::Ordering::Less => NSComparisonResult::Ascending,
                    std::cmp::Ordering::Equal => NSComparisonResult::Same,
                    std::cmp::Ordering::Greater => NSComparisonResult::Descending,
                }
            })
        }

        #[unsafe(method(offsetFromPosition:toPosition:))]
        fn offset_from_position(&self, from: &UITextPosition, to: &UITextPosition) -> NSInteger {
            os_entry("FluiTextInputView offsetFromPosition:toPosition:", (), AfterPanic::Answer(|()| 0), || {
                let (Some(from), Some(to)) = (offset(from), offset(to)) else { return 0 };
                let distance = |a: usize, b: usize| NSInteger::try_from(a - b).unwrap_or(NSInteger::MAX);
                if to >= from { distance(to, from) } else { -distance(from, to) }
            })
        }

        #[unsafe(method_id(inputDelegate))]
        fn input_delegate(&self) -> Option<Retained<ProtocolObject<dyn UITextInputDelegate>>> {
            os_entry("FluiTextInputView inputDelegate", (), AfterPanic::Answer(|()| None), || {
                self.ivars().input_delegate.borrow().as_ref().and_then(ObjcWeak::load)
            })
        }

        #[unsafe(method(setInputDelegate:))]
        fn set_input_delegate(&self, delegate: Option<&ProtocolObject<dyn UITextInputDelegate>>) {
            os_entry("FluiTextInputView setInputDelegate:", (), AfterPanic::Answer(|()| ()), || {
                *self.ivars().input_delegate.borrow_mut() = delegate.map(ObjcWeak::new);
            });
        }

        #[unsafe(method_id(tokenizer))]
        fn tokenizer(&self) -> Retained<ProtocolObject<dyn UITextInputTokenizer>> {
            os_entry("FluiTextInputView tokenizer", (), AfterPanic::Abort, || {
                let tokenizer = self.ivars().tokenizer.get_or_init(|| {
                    let responder: &UIResponder = self;
                    // SAFETY: the tokenizer keeps the text input it reads;
                    // the view outlives it, holding it in its own ivar.
                    unsafe {
                        UITextInputStringTokenizer::initWithTextInput(
                            UITextInputStringTokenizer::alloc(self.mtm()),
                            responder,
                        )
                    }
                });
                ProtocolObject::from_retained(tokenizer.clone())
            })
        }

        #[unsafe(method_id(positionWithinRange:farthestInDirection:))]
        fn position_within_range(&self, range: &UITextRange, direction: UITextLayoutDirection) -> Option<Retained<UITextPosition>> {
            os_entry("FluiTextInputView positionWithinRange:farthestInDirection:", (), AfterPanic::Answer(|()| None), || {
                let range = ours(range)?;
                let at = if backward(direction) { range.start } else { range.end };
                Some(Retained::into_super(TextPosition::new(self.mtm(), at)))
            })
        }

        #[unsafe(method_id(characterRangeByExtendingPosition:inDirection:))]
        fn character_range_by_extending(&self, position: &UITextPosition, direction: UITextLayoutDirection) -> Option<Retained<UITextRange>> {
            os_entry("FluiTextInputView characterRangeByExtendingPosition:inDirection:", (), AfterPanic::Answer(|()| None), || {
                let at = offset(position)?;
                let document = self.ivars().document.borrow();
                let range = if backward(direction) {
                    document.clamp_range(at.saturating_sub(1)..at)
                } else {
                    document.clamp_range(at..at.saturating_add(1))
                };
                Some(Retained::into_super(TextRange::new(self.mtm(), range)))
            })
        }

        #[unsafe(method(baseWritingDirectionForPosition:inDirection:))]
        fn base_writing_direction(&self, _position: &UITextPosition, _direction: UITextStorageDirection) -> NSWritingDirection {
            NSWritingDirection::Natural
        }

        #[unsafe(method(setBaseWritingDirection:forRange:))]
        fn set_base_writing_direction(&self, _direction: NSWritingDirection, _range: &UITextRange) {}

        #[unsafe(method(firstRectForRange:))]
        fn first_rect_for_range(&self, _range: &UITextRange) -> CGRect {
            os_entry("FluiTextInputView firstRectForRange:", (), AfterPanic::Answer(|()| CGRect::ZERO), || {
                self.ivars().caret.get()
            })
        }

        #[unsafe(method(caretRectForPosition:))]
        fn caret_rect_for_position(&self, _position: &UITextPosition) -> CGRect {
            os_entry("FluiTextInputView caretRectForPosition:", (), AfterPanic::Answer(|()| CGRect::ZERO), || {
                self.ivars().caret.get()
            })
        }

        #[unsafe(method_id(selectionRectsForRange:))]
        fn selection_rects_for_range(&self, _range: &UITextRange) -> Retained<NSArray<UITextSelectionRect>> {
            os_entry("FluiTextInputView selectionRectsForRange:", (), AfterPanic::Abort, NSArray::new)
        }

        #[unsafe(method_id(closestPositionToPoint:))]
        fn closest_position_to_point(&self, _point: CGPoint) -> Option<Retained<UITextPosition>> {
            os_entry("FluiTextInputView closestPositionToPoint:", (), AfterPanic::Answer(|()| None), || {
                let caret = self.ivars().document.borrow().selection().end;
                Some(Retained::into_super(TextPosition::new(self.mtm(), caret)))
            })
        }

        #[unsafe(method_id(closestPositionToPoint:withinRange:))]
        fn closest_position_within_range(&self, _point: CGPoint, range: &UITextRange) -> Option<Retained<UITextPosition>> {
            os_entry("FluiTextInputView closestPositionToPoint:withinRange:", (), AfterPanic::Answer(|()| None), || {
                let end = ours(range)?.end;
                Some(Retained::into_super(TextPosition::new(self.mtm(), end)))
            })
        }

        #[unsafe(method_id(characterRangeAtPoint:))]
        fn character_range_at_point(&self, _point: CGPoint) -> Option<Retained<UITextRange>> {
            None
        }
    }
);

impl TextInputView {
    /// A responder that tells `delivery`'s window what the keyboard types.
    pub(super) fn new(mtm: MainThreadMarker, delivery: Weak<WindowDelivery>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ResponderIvars {
            document: RefCell::new(Composition::default()),
            delivery,
            caret: Cell::new(CGRect::ZERO),
            tokenizer: OnceCell::new(),
            input_delegate: RefCell::new(None),
        });
        // SAFETY: UIView's designated initializer, on a fresh allocation.
        unsafe { msg_send![super(this), initWithFrame: CGRect::ZERO] }
    }

    /// The focused field's caret moved to `caret`, in the window's logical
    /// points.
    pub(super) fn set_caret(&self, caret: CGRect) {
        self.ivars().caret.set(caret);
    }

    /// The field lost focus, or composition was turned off: the document
    /// empties, and a composition in progress ends uncommitted. The keyboard
    /// is told the text changed under it, as UIKit asks of a change it did
    /// not make.
    pub(super) fn reset(&self) {
        let delegate = self
            .ivars()
            .input_delegate
            .borrow()
            .as_ref()
            .and_then(ObjcWeak::load);
        let input = ProtocolObject::<dyn UITextInput>::from_ref(self);
        if let Some(delegate) = &delegate {
            delegate.textWillChange(Some(input));
        }
        let edit = self.ivars().document.borrow_mut().reset();
        if let Some(delegate) = &delegate {
            delegate.textDidChange(Some(input));
        }
        self.announce("reset", edit);
    }

    /// The position `by` units from `from`, if it lies in the document.
    fn moved(&self, from: usize, by: NSInteger) -> Option<Retained<UITextPosition>> {
        let to = from.checked_add_signed(by)?;
        (to <= self.ivars().document.borrow().len())
            .then(|| Retained::into_super(TextPosition::new(self.mtm(), to)))
    }

    /// Tell the window what `edit` means for the focused field, then ask for
    /// a frame. The document is not borrowed: the field's code runs here.
    fn announce(&self, method: &'static str, edit: Option<Edit>) {
        let (marked, selection) = {
            let document = self.ivars().document.borrow();
            (document.marked(), document.selection())
        };
        // In the message, not as fields: the system log shows a message but
        // redacts the fields of an app's entry.
        tracing::debug!(
            target: LOG,
            "keyboard edit: {method} {edit:?} marked={marked:?} selection={selection:?}"
        );
        let Some(edit) = edit else { return };
        let Some(delivery) = self.ivars().delivery.upgrade() else {
            return;
        };
        match edit {
            Edit::Preedit { text, cursor } => {
                delivery.dispatch_input(PlatformInput::Ime(ImeEvent::Preedit { text, cursor }));
            }
            Edit::Commit(text) => {
                delivery.dispatch_input(PlatformInput::Ime(ImeEvent::Commit(text)));
            }
            Edit::DeleteBackward => {
                for state in [KeyState::Down, KeyState::Up] {
                    delivery.dispatch_input(PlatformInput::Keyboard(KeyEvent::new(
                        state,
                        Key::Named(NamedKey::Backspace),
                        Code::Backspace,
                        EventTime::from_nanos(crate::input::event_timestamp_ns()),
                    )));
                }
            }
        }
        delivery.dispatch_request_frame();
    }
}

/// The range of one of this view's ranges; `None` for one UIKit made.
fn ours(range: &UITextRange) -> Option<Range<usize>> {
    range.downcast_ref::<TextRange>().map(TextRange::range)
}

/// The offset of one of this view's positions; `None` for one UIKit made.
fn offset(position: &UITextPosition) -> Option<usize> {
    position
        .downcast_ref::<TextPosition>()
        .map(TextPosition::offset)
}

/// Whether `direction` runs toward the document's start.
fn backward(direction: UITextLayoutDirection) -> bool {
    direction == UITextLayoutDirection::Left || direction == UITextLayoutDirection::Up
}

fn utf16(text: &NSString) -> Vec<u16> {
    text.to_string().encode_utf16().collect()
}
