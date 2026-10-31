//! The responder the system keyboard types into (ADR-0255 decision 11): an
//! invisible `UITextInput` view beside the window's `FluiView`, which never
//! becomes first responder itself. It serves the focused field's store to the
//! keyboard through the window's [`TextClient`], the store host AppKit's
//! text input serves it through too (ADR-0090): each `UITextInput` method is
//! one of the client's verbs, and the client tells the keyboard which field
//! it serves and what the application changed ([`ResponderContext`]).
//!
//! UIKit asks a `UITextInput` for positions and ranges as objects of its own
//! making: [`TextPosition`] and [`TextRange`] carry an offset into the
//! field's document, in UTF-16 units, as the store counts.
//!
//! The view sits at the window view's origin, so the store's logical window
//! coordinates are its own.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::sync::Weak;

use flui_foundation::geometry::{Bounds, Point};
use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState, NamedKey};
use flui_platform::text_store::TextStoreStatus;
use flui_platform::text_store::utf16::{Utf16Offset, Utf16Range};
use flui_platform::{EventTime, PlatformInput};
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};

use objc2_foundation::{
    NSArray, NSAttributedStringKey, NSComparisonResult, NSDictionary, NSInteger, NSObject,
    NSObjectProtocol, NSPoint as CGPoint, NSRange, NSRect as CGRect, NSSize as CGSize, NSString,
};
use objc2_ui_kit::{
    NSWritingDirection, UIKeyInput, UIKeyboardType, UIResponder, UITextAutocorrectionType,
    UITextInput, UITextInputDelegate, UITextInputStringTokenizer, UITextInputTokenizer,
    UITextInputTraits, UITextLayoutDirection, UITextPosition, UITextRange, UITextSelectionRect,
    UITextSpellCheckingType, UITextStorageDirection, UIView,
};

use super::super::client::{DocumentChange, InputContext, TextClient, Typed};
use super::super::traits::{KeyboardTraits, keyboard_traits};
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use crate::owner::window_delivery::WindowDelivery;

/// What the target of the keyboard's log lines is.
const LOG: &str = "flui.text.ios";

define_class!(
    /// An offset into a [`TextInputView`]'s document.
    #[unsafe(super(UITextPosition, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FluiTextPosition"]
    #[ivars = Utf16Offset]
    pub(super) struct TextPosition;
);

impl TextPosition {
    fn new(mtm: MainThreadMarker, offset: Utf16Offset) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(offset);
        // SAFETY: NSObject's designated initializer, on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }

    fn offset(&self) -> Utf16Offset {
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
    fn new(mtm: MainThreadMarker, range: Utf16Range) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ends {
            start: TextPosition::new(mtm, range.start()),
            end: TextPosition::new(mtm, range.end()),
        });
        // SAFETY: NSObject's designated initializer, on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }

    fn range(&self) -> Utf16Range {
        Utf16Range::try_new(self.ivars().start.offset(), self.ivars().end.offset())
            .expect("BUG: a range this view made starts first")
    }
}

/// What a [`TextInputView`] keeps.
pub(super) struct ResponderIvars {
    /// The window's store host, which serves the focused field.
    client: Rc<TextClient>,
    /// The window the Return and Backspace presses are told to.
    delivery: Weak<WindowDelivery>,
    /// What the keyboard is told of the focused field.
    traits: Cell<KeyboardTraits>,
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

    /// The focused field's traits ([`keyboard_traits`]): UIKit reads them as
    /// the view becomes first responder and again on `reloadInputViews`.
    unsafe impl UITextInputTraits for TextInputView {
        #[unsafe(method(isSecureTextEntry))]
        fn is_secure_text_entry(&self) -> bool {
            // A field whose traits cannot be read is taken for a password.
            os_entry(
                "FluiTextInputView isSecureTextEntry",
                (),
                AfterPanic::Answer(|()| true),
                || self.ivars().traits.get().secure,
            )
        }

        #[unsafe(method(keyboardType))]
        fn keyboard_type(&self) -> UIKeyboardType {
            os_entry(
                "FluiTextInputView keyboardType",
                (),
                AfterPanic::Answer(|()| UIKeyboardType::Default),
                || UIKeyboardType(self.ivars().traits.get().keyboard),
            )
        }

        #[unsafe(method(autocorrectionType))]
        fn autocorrection_type(&self) -> UITextAutocorrectionType {
            os_entry(
                "FluiTextInputView autocorrectionType",
                (),
                AfterPanic::Answer(|()| UITextAutocorrectionType::No),
                || UITextAutocorrectionType(self.ivars().traits.get().autocorrection),
            )
        }

        #[unsafe(method(spellCheckingType))]
        fn spell_checking_type(&self) -> UITextSpellCheckingType {
            os_entry(
                "FluiTextInputView spellCheckingType",
                (),
                AfterPanic::Answer(|()| UITextSpellCheckingType::No),
                || UITextSpellCheckingType(self.ivars().traits.get().spell_checking),
            )
        }
    }

    unsafe impl UIKeyInput for TextInputView {
        /// Always yes: a no would stop the keyboard sending the delete a
        /// field the store could not be read for still needs. A delete with
        /// nothing there does nothing.
        #[unsafe(method(hasText))]
        fn has_text(&self) -> bool {
            true
        }

        #[unsafe(method(insertText:))]
        fn insert_text(&self, text: &NSString) {
            os_entry("FluiTextInputView insertText:", (), AfterPanic::Answer(|()| ()), || {
                if self.client().type_text(text.to_string()) == Typed::Return {
                    self.press(NamedKey::Enter, Code::Enter);
                }
            });
        }

        /// A Backspace press: the field deletes the grapheme cluster before
        /// its caret, or its selection, as it does for a hardware key.
        #[unsafe(method(deleteBackward))]
        fn delete_backward(&self) {
            os_entry("FluiTextInputView deleteBackward", (), AfterPanic::Answer(|()| ()), || {
                self.press(NamedKey::Backspace, Code::Backspace);
            });
        }
    }

    unsafe impl UITextInput for TextInputView {
        #[unsafe(method_id(textInRange:))]
        fn text_in_range(&self, range: &UITextRange) -> Option<Retained<NSString>> {
            os_entry("FluiTextInputView textInRange:", (), AfterPanic::Answer(|()| None), || {
                let (text, _) = self.client().substring(ours(range)?)?;
                Some(NSString::from_str(&text))
            })
        }

        #[unsafe(method(replaceRange:withText:))]
        fn replace_range(&self, range: &UITextRange, text: &NSString) {
            os_entry("FluiTextInputView replaceRange:withText:", (), AfterPanic::Answer(|()| ()), || {
                if let Some(range) = ours(range) {
                    self.client().replace(range, text.to_string());
                }
            });
        }

        #[unsafe(method_id(selectedTextRange))]
        fn selected_text_range(&self) -> Option<Retained<UITextRange>> {
            os_entry("FluiTextInputView selectedTextRange", (), AfterPanic::Answer(|()| None), || {
                let selection = self.client().selected_range()?;
                Some(Retained::into_super(TextRange::new(self.mtm(), selection)))
            })
        }

        #[unsafe(method(setSelectedTextRange:))]
        fn set_selected_text_range(&self, range: Option<&UITextRange>) {
            os_entry("FluiTextInputView setSelectedTextRange:", (), AfterPanic::Answer(|()| ()), || {
                if let Some(range) = range.and_then(ours) {
                    self.client().select(range);
                }
            });
        }

        #[unsafe(method_id(markedTextRange))]
        fn marked_text_range(&self) -> Option<Retained<UITextRange>> {
            os_entry("FluiTextInputView markedTextRange", (), AfterPanic::Answer(|()| None), || {
                let marked = self.client().marked_range()?;
                Some(Retained::into_super(TextRange::new(self.mtm(), marked)))
            })
        }

        #[unsafe(method_id(markedTextStyle))]
        fn marked_text_style(&self) -> Option<Retained<NSDictionary<NSAttributedStringKey, objc2::runtime::AnyObject>>> {
            None
        }

        #[unsafe(method(setMarkedTextStyle:))]
        fn set_marked_text_style(&self, _style: Option<&NSDictionary<NSAttributedStringKey, objc2::runtime::AnyObject>>) {}

        /// The marked text replaces the composition, else the selection;
        /// `selected` is the selection within it.
        #[unsafe(method(setMarkedText:selectedRange:))]
        fn set_marked_text(&self, marked: Option<&NSString>, selected: NSRange) {
            os_entry("FluiTextInputView setMarkedText:selectedRange:", (), AfterPanic::Answer(|()| ()), || {
                let text = marked.map(NSString::to_string).unwrap_or_default();
                let end = selected.location.saturating_add(selected.length);
                let selected = Utf16Range::try_new(Utf16Offset::new(selected.location), Utf16Offset::new(end))
                    .expect("BUG: a saturating end is never before its start");
                self.client().set_marked_text(text, selected, None);
            });
        }

        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            os_entry("FluiTextInputView unmarkText", (), AfterPanic::Answer(|()| ()), || {
                self.client().unmark_text();
            });
        }

        #[unsafe(method_id(beginningOfDocument))]
        fn beginning_of_document(&self) -> Retained<UITextPosition> {
            os_entry("FluiTextInputView beginningOfDocument", (), AfterPanic::Abort, || {
                Retained::into_super(TextPosition::new(self.mtm(), Utf16Offset::ZERO))
            })
        }

        #[unsafe(method_id(endOfDocument))]
        fn end_of_document(&self) -> Retained<UITextPosition> {
            os_entry("FluiTextInputView endOfDocument", (), AfterPanic::Abort, || {
                Retained::into_super(TextPosition::new(self.mtm(), self.end()))
            })
        }

        #[unsafe(method_id(textRangeFromPosition:toPosition:))]
        fn text_range_from_position(&self, from: &UITextPosition, to: &UITextPosition) -> Option<Retained<UITextRange>> {
            os_entry("FluiTextInputView textRangeFromPosition:toPosition:", (), AfterPanic::Answer(|()| None), || {
                let range = self.range(offset(from)?, offset(to)?);
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
                let distance = |a: Utf16Offset, b: Utf16Offset| {
                    NSInteger::try_from(a.get() - b.get()).unwrap_or(NSInteger::MAX)
                };
                if to >= from { distance(to, from) } else { -distance(from, to) }
            })
        }

        #[unsafe(method_id(inputDelegate))]
        fn input_delegate(&self) -> Option<Retained<ProtocolObject<dyn UITextInputDelegate>>> {
            os_entry("FluiTextInputView inputDelegate", (), AfterPanic::Answer(|()| None), || {
                self.delegate()
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
                    // SAFETY: `initWithTextInput:` requires a responder that
                    // implements `UITextInput`, and `self` is a
                    // `FluiTextInputView`, which does (`unsafe impl
                    // UITextInput` above). The tokenizer holds its text input
                    // in a weak ivar (`_textInput`, the class's weak ivar
                    // layout; the view's retain count is unchanged by it, as
                    // read on iOS 27.1), so the view in its ivar makes no
                    // cycle, and a tokenizer UIKit keeps past the view finds
                    // no text input rather than a freed one.
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
                let at = if backward(direction) { range.start() } else { range.end() };
                Some(Retained::into_super(TextPosition::new(self.mtm(), at)))
            })
        }


        #[unsafe(method_id(characterRangeByExtendingPosition:inDirection:))]
        fn character_range_by_extending(&self, position: &UITextPosition, direction: UITextLayoutDirection) -> Option<Retained<UITextRange>> {
            os_entry("FluiTextInputView characterRangeByExtendingPosition:inDirection:", (), AfterPanic::Answer(|()| None), || {
                let range = self.extended(offset(position)?, backward(direction));
                Some(Retained::into_super(TextRange::new(self.mtm(), range)))
            })
        }

        #[unsafe(method(baseWritingDirectionForPosition:inDirection:))]
        fn base_writing_direction(&self, _position: &UITextPosition, _direction: UITextStorageDirection) -> NSWritingDirection {
            NSWritingDirection::Natural
        }

        #[unsafe(method(setBaseWritingDirection:forRange:))]
        fn set_base_writing_direction(&self, _direction: NSWritingDirection, _range: &UITextRange) {}

        /// The range's first line, from the field's layout: where the
        /// candidate bar and the dictation marker go.
        #[unsafe(method(firstRectForRange:))]
        fn first_rect_for_range(&self, range: &UITextRange) -> CGRect {
            os_entry("FluiTextInputView firstRectForRange:", (), AfterPanic::Answer(|()| CGRect::ZERO), || {
                self.rect(ours(range))
            })
        }

        #[unsafe(method(caretRectForPosition:))]
        fn caret_rect_for_position(&self, position: &UITextPosition) -> CGRect {
            os_entry("FluiTextInputView caretRectForPosition:", (), AfterPanic::Answer(|()| CGRect::ZERO), || {
                let at = offset(position).map(|at| self.range(at, at));
                self.rect(at)
            })
        }

        #[unsafe(method_id(selectionRectsForRange:))]
        fn selection_rects_for_range(&self, _range: &UITextRange) -> Retained<NSArray<UITextSelectionRect>> {
            os_entry("FluiTextInputView selectionRectsForRange:", (), AfterPanic::Abort, NSArray::new)
        }

        #[unsafe(method_id(closestPositionToPoint:))]
        fn closest_position_to_point(&self, point: CGPoint) -> Option<Retained<UITextPosition>> {
            os_entry("FluiTextInputView closestPositionToPoint:", (), AfterPanic::Answer(|()| None), || {
                let at = self.client().character_index(Point::new(point.x, point.y))?;
                Some(Retained::into_super(TextPosition::new(self.mtm(), at)))
            })
        }

        #[unsafe(method_id(closestPositionToPoint:withinRange:))]
        fn closest_position_within_range(&self, point: CGPoint, range: &UITextRange) -> Option<Retained<UITextPosition>> {
            os_entry("FluiTextInputView closestPositionToPoint:withinRange:", (), AfterPanic::Answer(|()| None), || {
                let range = ours(range)?;
                let at = self
                    .client()
                    .character_index(Point::new(point.x, point.y))
                    .unwrap_or(range.end())
                    .clamp(range.start(), range.end());
                Some(Retained::into_super(TextPosition::new(self.mtm(), at)))
            })
        }

        #[unsafe(method_id(characterRangeAtPoint:))]
        fn character_range_at_point(&self, point: CGPoint) -> Option<Retained<UITextRange>> {
            os_entry("FluiTextInputView characterRangeAtPoint:", (), AfterPanic::Answer(|()| None), || {
                let at = self.client().character_index(Point::new(point.x, point.y))?;
                Some(Retained::into_super(TextRange::new(self.mtm(), self.extended(at, false))))
            })
        }
    }
);

impl TextInputView {
    /// A responder serving the field the window's store host focuses, which
    /// tells `delivery`'s window the Return and Backspace presses.
    pub(super) fn new(mtm: MainThreadMarker, delivery: Weak<WindowDelivery>) -> Retained<Self> {
        let client = TextClient::new();
        let this = Self::alloc(mtm).set_ivars(ResponderIvars {
            client: Rc::clone(&client),
            delivery,
            traits: Cell::new(KeyboardTraits::DEFAULT),
            tokenizer: OnceCell::new(),
            input_delegate: RefCell::new(None),
        });
        // SAFETY: UIView's designated initializer, on a fresh allocation.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: CGRect::ZERO] };
        // The context holds the view weakly: the view holds the client.
        client.bind(Rc::new(ResponderContext {
            view: ObjcWeak::from_retained(&this),
        }));
        this
    }

    /// The window's store host: the client serving the focused field.
    pub(super) fn client(&self) -> &Rc<TextClient> {
        &self.ivars().client
    }

    fn delegate(&self) -> Option<Retained<ProtocolObject<dyn UITextInputDelegate>>> {
        self.ivars()
            .input_delegate
            .borrow()
            .as_ref()
            .and_then(ObjcWeak::load)
    }

    /// The end of the focused field's document; its start when no field has
    /// focus or the store cannot be read now.
    fn end(&self) -> Utf16Offset {
        self.client().document_len().unwrap_or(Utf16Offset::ZERO)
    }

    /// The range between `a` and `b`, in order and inside the document.
    fn range(&self, a: Utf16Offset, b: Utf16Offset) -> Utf16Range {
        let end = self.end();
        let (a, b) = (a.min(end), b.min(end));
        Utf16Range::try_new(a.min(b), a.max(b)).expect("BUG: min and max are ordered")
    }

    /// The position `by` units from `from`, if it lies in the document.
    fn moved(&self, from: Utf16Offset, by: NSInteger) -> Option<Retained<UITextPosition>> {
        let to = Utf16Offset::new(from.get().checked_add_signed(by)?);
        (to <= self.end()).then(|| Retained::into_super(TextPosition::new(self.mtm(), to)))
    }

    /// The unit before `at` (`backward`) or after it, inside the document;
    /// empty at its end.
    fn extended(&self, at: Utf16Offset, backward: bool) -> Utf16Range {
        let other = if backward {
            Utf16Offset::new(at.get().saturating_sub(1))
        } else {
            Utf16Offset::new(at.get().saturating_add(1))
        };
        self.range(at, other)
    }

    /// The rectangle of `range`'s first line in this view's points, from the
    /// field's layout; empty where no field has focus or its layout has none.
    fn rect(&self, range: Option<Utf16Range>) -> CGRect {
        let Some((bounds, _)) = range.and_then(|range| self.client().first_rect(Some(range)))
        else {
            return CGRect::ZERO;
        };
        let Bounds { origin, size } = bounds;
        CGRect::new(
            CGPoint::new(origin.x, origin.y),
            CGSize::new(size.width, size.height),
        )
    }

    /// A press and release of `key`, told to the window as a hardware key
    /// would be, then a frame asked for.
    fn press(&self, key: NamedKey, code: Code) {
        tracing::debug!(target: LOG, "keyboard key: {key:?}");
        let Some(delivery) = self.ivars().delivery.upgrade() else {
            return;
        };
        for state in [KeyState::Down, KeyState::Up] {
            delivery.dispatch_input(PlatformInput::Keyboard(KeyEvent::new(
                state,
                Key::Named(key),
                code,
                EventTime::from_nanos(crate::input::event_timestamp_ns()),
            )));
        }
        delivery.dispatch_request_frame();
    }

    /// The keyboard serves `status`'s field: its traits, re-read by a
    /// keyboard already up; the keyboard comes up for a field and goes down
    /// without one.
    fn serve(&self, status: Option<TextStoreStatus>) {
        let Some(status) = status else {
            if self.isFirstResponder() {
                self.resignFirstResponder();
            }
            return;
        };
        let traits = keyboard_traits(status);
        let changed = self.ivars().traits.replace(traits) != traits;
        if !self.isFirstResponder() {
            self.becomeFirstResponder();
        } else if changed {
            self.reloadInputViews();
        }
    }

    /// Tell the keyboard the document changed under it, which it did not
    /// do: it drops its marked text and reads the field again.
    fn text_changed_under_keyboard(&self) {
        let delegate = self.delegate();
        let input = ProtocolObject::<dyn UITextInput>::from_ref(self);
        if let Some(delegate) = &delegate {
            delegate.textWillChange(Some(input));
            delegate.textDidChange(Some(input));
        }
    }

    /// Tell the keyboard the selection moved under it.
    fn selection_changed_under_keyboard(&self) {
        let delegate = self.delegate();
        let input = ProtocolObject::<dyn UITextInput>::from_ref(self);
        if let Some(delegate) = &delegate {
            delegate.selectionWillChange(Some(input));
            delegate.selectionDidChange(Some(input));
        }
    }
}

/// The keyboard as the window's [`TextClient`] reaches it, through the
/// responder.
struct ResponderContext {
    /// The responder, held weakly: it holds the client, which holds this.
    view: ObjcWeak<TextInputView>,
}

impl InputContext for ResponderContext {
    /// UIKit has no call that drops the keyboard's marked text alone: told
    /// the text changed under it, it drops it.
    fn discard_marked_text(&self) {
        if let Some(view) = self.view.load() {
            view.text_changed_under_keyboard();
        }
    }

    /// UIKit asks `firstRectForRange:` each time it places the candidate
    /// bar, and keeps no rectangle to invalidate.
    fn invalidate_character_coordinates(&self) {}

    fn field_changed(&self, status: Option<TextStoreStatus>) {
        if let Some(view) = self.view.load() {
            view.serve(status);
        }
    }

    fn document_changed(&self, change: DocumentChange) {
        if let Some(view) = self.view.load() {
            match change {
                DocumentChange::Text => view.text_changed_under_keyboard(),
                DocumentChange::Selection => view.selection_changed_under_keyboard(),
            }
        }
    }
}

/// The range of one of this view's ranges; `None` for one UIKit made.
fn ours(range: &UITextRange) -> Option<Utf16Range> {
    range.downcast_ref::<TextRange>().map(TextRange::range)
}

/// The offset of one of this view's positions; `None` for one UIKit made.
fn offset(position: &UITextPosition) -> Option<Utf16Offset> {
    position
        .downcast_ref::<TextPosition>()
        .map(TextPosition::offset)
}

/// Whether `direction` runs toward the document's start.
fn backward(direction: UITextLayoutDirection) -> bool {
    direction == UITextLayoutDirection::Left || direction == UITextLayoutDirection::Up
}
