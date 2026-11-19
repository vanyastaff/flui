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
//! field's document, in UTF-16 units, as the store counts, and
//! [`TextSelectionRect`] one line of a range's selection.
//!
//! While a field is served, a `UITextInteraction` in editable mode on the
//! window's view gives it UIKit's selection gestures, handles, loupe and edit
//! menu, beginning only over the field (ADR-0255 decision 11).
//!
//! The view sits at the window view's origin, so the store's logical window
//! coordinates are its own.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::sync::Weak;

use flui_foundation::TextDirection;
use flui_foundation::geometry::{Bounds, Point};
use flui_platform::input::keyboard::{Code, Key, KeyEvent, KeyPress, KeyState, NamedKey};
use flui_platform::text::store::utf16::{Utf16Offset, Utf16Range};
use flui_platform::text::store::{Autofill, EditCommand, PressTarget, TextStoreStatus};
use flui_platform::{EventTime, PlatformInput};
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};

use objc2_foundation::{
    NSArray, NSAttributedStringKey, NSComparisonResult, NSDictionary, NSInteger, NSObject,
    NSObjectProtocol, NSPoint as CGPoint, NSRange, NSRect as CGRect, NSSize as CGSize, NSString,
};
use objc2_ui_kit::{
    NSWritingDirection, UIEditMenuInteraction, UIInteraction, UIKeyInput, UIKeyboardType,
    UIPasteboard, UIResponder, UIReturnKeyType, UITextAutocapitalizationType,
    UITextAutocorrectionType, UITextContentTypeNewPassword, UITextContentTypeOneTimeCode,
    UITextContentTypePassword, UITextContentTypeUsername, UITextInput, UITextInputDelegate,
    UITextInputStringTokenizer, UITextInputTokenizer, UITextInputTraits, UITextInteraction,
    UITextInteractionDelegate, UITextInteractionMode, UITextLayoutDirection, UITextPosition,
    UITextRange, UITextSelectionRect, UITextSpellCheckingType, UITextStorageDirection, UIView,
};

use super::super::client::{DocumentChange, InputContext, MarkedText, TextClient, Typed};
use super::super::selection_rects::{SelectionRect, gesture_begins};
use super::super::traits::{KeyboardTraits, keyboard_traits};
use crate::owner::panic_boundary::{AfterPanic, ContainedIvars, os_entry};
use crate::owner::window_delivery::WindowDelivery;

/// What the target of the keyboard's log lines is.
const LOG: &str = "flui.text.ios";

/// How far around the edge a selection handle stands on, in points, UIKit's
/// selection gestures still begin: half of the 44-point touch target Apple's
/// Human Interface Guidelines ask for, so a touch on the handle's knob, which
/// UIKit draws past the line it ends and may lie outside the field's box,
/// still reaches the handle. Only around the handles: a band around the whole
/// field would take a tap on a control beside it.
const HANDLE_REACH: f64 = 22.0;

define_class!(
    /// An offset into a [`TextInputView`]'s document.
    #[unsafe(super(UITextPosition, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FluiTextPosition"]
    #[ivars = ContainedIvars<Utf16Offset>]
    pub(super) struct TextPosition;
);

impl TextPosition {
    fn new(mtm: MainThreadMarker, offset: Utf16Offset) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ContainedIvars::new(offset));
        // SAFETY: NSObject's designated initializer, on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }

    fn offset(&self) -> Utf16Offset {
        **self.ivars()
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
    #[ivars = ContainedIvars<Ends>]
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
        let this = Self::alloc(mtm).set_ivars(ContainedIvars::new(Ends {
            start: TextPosition::new(mtm, range.start()),
            end: TextPosition::new(mtm, range.end()),
        }));
        // SAFETY: NSObject's designated initializer, on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }

    fn range(&self) -> Utf16Range {
        Utf16Range::try_new(self.ivars().start.offset(), self.ivars().end.offset())
            .expect("BUG: a range this view made starts first")
    }
}

define_class!(
    /// One line of a range of a [`TextInputView`]'s document: UIKit draws
    /// the selection's highlight from these and puts its handles at the
    /// leading edge of the one that holds the start and the trailing edge of
    /// the one that holds the end.
    #[unsafe(super(UITextSelectionRect, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FluiTextSelectionRect"]
    #[ivars = ContainedIvars<SelectionRect>]
    pub(super) struct TextSelectionRect;

    impl TextSelectionRect {
        #[unsafe(method(rect))]
        fn rect(&self) -> CGRect {
            os_entry("FluiTextSelectionRect rect", (), AfterPanic::Answer(|()| CGRect::ZERO), || {
                cg_rect(self.ivars().bounds)
            })
        }

        #[unsafe(method(writingDirection))]
        fn writing_direction(&self) -> NSWritingDirection {
            os_entry(
                "FluiTextSelectionRect writingDirection",
                (),
                AfterPanic::Answer(|()| NSWritingDirection::Natural),
                || match self.ivars().direction {
                    TextDirection::Ltr => NSWritingDirection::LeftToRight,
                    TextDirection::Rtl => NSWritingDirection::RightToLeft,
                },
            )
        }

        #[unsafe(method(containsStart))]
        fn contains_start(&self) -> bool {
            os_entry("FluiTextSelectionRect containsStart", (), AfterPanic::Answer(|()| false), || {
                self.ivars().contains_start
            })
        }

        #[unsafe(method(containsEnd))]
        fn contains_end(&self) -> bool {
            os_entry("FluiTextSelectionRect containsEnd", (), AfterPanic::Answer(|()| false), || {
                self.ivars().contains_end
            })
        }

        /// Fields lay text out horizontally.
        #[unsafe(method(isVertical))]
        fn is_vertical(&self) -> bool {
            false
        }
    }
);

impl TextSelectionRect {
    fn new(mtm: MainThreadMarker, rect: SelectionRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ContainedIvars::new(rect));
        // SAFETY: NSObject's designated initializer, on a fresh allocation.
        unsafe { msg_send![super(this), init] }
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
    /// The traits changed while the keyboard composed: reloaded once its
    /// marked text ends, never under it.
    reload_owed: Cell<bool>,
    tokenizer: OnceCell<Retained<UITextInputStringTokenizer>>,
    input_delegate: RefCell<Option<ObjcWeak<ProtocolObject<dyn UITextInputDelegate>>>>,
    /// UIKit's selection gestures, handles, loupe and edit menu over the
    /// focused field: set once the view exists, and on the window's view
    /// exactly while a field is served ([`TextInputView::serve`]).
    interaction: OnceCell<Retained<UITextInteraction>>,
}

define_class!(
    /// The invisible view the system keyboard types into.
    #[unsafe(super(UIView, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FluiTextInputView"]
    #[ivars = ContainedIvars<ResponderIvars>]
    pub(super) struct TextInputView;

    impl TextInputView {
        #[unsafe(method(canBecomeFirstResponder))]
        fn can_become_first_responder(&self) -> bool {
            true
        }

        /// The edit menu's Cut, Copy, Paste, Select and Select All, offered
        /// when the focused field would perform them, Paste only while the
        /// pasteboard holds text (`hasStrings`, which reads no content and so
        /// shows no paste prompt); any other action is UIKit's to answer.
        #[unsafe(method(canPerformAction:withSender:))]
        fn can_perform_action(&self, action: Sel, sender: Option<&AnyObject>) -> bool {
            os_entry("FluiTextInputView canPerformAction:withSender:", (), AfterPanic::Answer(|()| false), || {
                match edit_command(action) {
                    Some(EditCommand::Paste) => {
                        self.client().edit_command_enabled(EditCommand::Paste) && pasteboard_has_text()
                    }
                    Some(command) => self.client().edit_command_enabled(command),
                    // SAFETY: UIResponder's `canPerformAction:withSender:`,
                    // with the selector and sender UIKit passed.
                    None => unsafe { msg_send![super(self), canPerformAction: action, withSender: sender] },
                }
            })
        }

        #[unsafe(method(select:))]
        fn select(&self, _sender: Option<&AnyObject>) {
            os_entry("FluiTextInputView select:", (), AfterPanic::Answer(|()| ()), || {
                self.client().perform_edit_command(EditCommand::SelectWord);
            });
        }

        #[unsafe(method(cut:))]
        fn cut(&self, _sender: Option<&AnyObject>) {
            os_entry("FluiTextInputView cut:", (), AfterPanic::Answer(|()| ()), || {
                self.client().perform_edit_command(EditCommand::Cut);
            });
        }

        #[unsafe(method(copy:))]
        fn copy(&self, _sender: Option<&AnyObject>) {
            os_entry("FluiTextInputView copy:", (), AfterPanic::Answer(|()| ()), || {
                self.client().perform_edit_command(EditCommand::Copy);
            });
        }

        #[unsafe(method(paste:))]
        fn paste(&self, _sender: Option<&AnyObject>) {
            os_entry("FluiTextInputView paste:", (), AfterPanic::Answer(|()| ()), || {
                self.client().perform_edit_command(EditCommand::Paste);
            });
        }

        #[unsafe(method(selectAll:))]
        fn select_all(&self, _sender: Option<&AnyObject>) {
            os_entry("FluiTextInputView selectAll:", (), AfterPanic::Answer(|()| ()), || {
                self.client().perform_edit_command(EditCommand::SelectAll);
            });
        }
    }

    // SAFETY: NSObjectProtocol has no additional requirements.
    unsafe impl NSObjectProtocol for TextInputView {}

    /// The focused field's traits ([`keyboard_traits`]): UIKit reads them as
    /// the view becomes first responder and again on `reloadInputViews`.
    // SAFETY: the methods below carry UIKit's selectors and signatures for
    // this protocol and run on the main thread, where UIKit calls them.
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

        #[unsafe(method(returnKeyType))]
        fn return_key_type(&self) -> UIReturnKeyType {
            os_entry(
                "FluiTextInputView returnKeyType",
                (),
                AfterPanic::Answer(|()| UIReturnKeyType::Default),
                || UIReturnKeyType(self.ivars().traits.get().return_key),
            )
        }

        #[unsafe(method(autocapitalizationType))]
        fn autocapitalization_type(&self) -> UITextAutocapitalizationType {
            os_entry(
                "FluiTextInputView autocapitalizationType",
                (),
                AfterPanic::Answer(|()| UITextAutocapitalizationType::None),
                || UITextAutocapitalizationType(self.ivars().traits.get().autocapitalization),
            )
        }

        /// UIKit's constant for what the field may be filled with, or `nil`.
        #[unsafe(method_id(textContentType))]
        fn text_content_type(&self) -> Option<Retained<NSString>> {
            os_entry(
                "FluiTextInputView textContentType",
                (),
                AfterPanic::Answer(|()| None),
                || {
                    let constant: &'static NSString =
                        match self.ivars().traits.get().content? {
                            // SAFETY: a UIKit constant, immutable for the process's life.
                            Autofill::Username => unsafe { UITextContentTypeUsername },
                            // SAFETY: a UIKit constant, immutable for the process's life.
                            Autofill::CurrentPassword => unsafe { UITextContentTypePassword },
                            // SAFETY: a UIKit constant, immutable for the process's life.
                            Autofill::NewPassword => unsafe { UITextContentTypeNewPassword },
                            // SAFETY: a UIKit constant, immutable for the process's life.
                            Autofill::OneTimeCode => unsafe { UITextContentTypeOneTimeCode },
                            // A kind added later that UIKit has no word for here.
                            _ => return None,
                        };
                    Some(constant.retain())
                },
            )
        }
    }

    // SAFETY: the methods below carry UIKit's selectors and signatures for
    // this protocol and run on the main thread, where UIKit calls them.
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
                let typed = self.client().type_text(text.to_string());
                self.reload_if_owed();
                if typed == Typed::Return {
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

    // SAFETY: the methods below carry UIKit's selectors and signatures for
    // this protocol and run on the main thread, where UIKit calls them.
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
                let text = marked.map_or_default(NSString::to_string);
                let end = selected.location.saturating_add(selected.length);
                let selected = Utf16Range::try_new(Utf16Offset::new(selected.location), Utf16Offset::new(end))
                    .expect("BUG: a saturating end is never before its start");
                self.client().set_marked_text(text, selected, None);
                self.reload_if_owed();
            });
        }

        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            os_entry("FluiTextInputView unmarkText", (), AfterPanic::Answer(|()| ()), || {
                self.client().unmark_text();
                self.reload_if_owed();
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
                let range = self.extended(offset(position)?, backward(direction))?;
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

        /// The caret, held inside the part of the field the window shows: one
        /// scrolled out stands at the nearest edge of that part
        /// ([`TextClient::caret_rect`]); an empty rectangle only when no
        /// field is served or it shows nothing.
        #[unsafe(method(caretRectForPosition:))]
        fn caret_rect_for_position(&self, position: &UITextPosition) -> CGRect {
            os_entry("FluiTextInputView caretRectForPosition:", (), AfterPanic::Answer(|()| CGRect::ZERO), || {
                offset(position)
                    .and_then(|at| self.client().caret_rect(at))
                    .map_or(CGRect::ZERO, cg_rect)
            })
        }

        /// One rectangle per line of the range, from the field's layout; none
        /// for a range UIKit made (a keyboard switching to Japanese asks
        /// with positions of its own) or a field with no layout.
        #[unsafe(method_id(selectionRectsForRange:))]
        fn selection_rects_for_range(&self, range: &UITextRange) -> Retained<NSArray<UITextSelectionRect>> {
            os_entry("FluiTextInputView selectionRectsForRange:", (), AfterPanic::Answer(|()| NSArray::new()), || {
                let Some(range) = ours(range) else {
                    return NSArray::new();
                };
                let rects: Vec<Retained<UITextSelectionRect>> = self
                    .client()
                    .selection_rects(range)
                    .into_iter()
                    .map(|rect| Retained::into_super(TextSelectionRect::new(self.mtm(), rect)))
                    .collect();
                NSArray::from_retained_slice(&rects)
            })
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
                let range = self.client().character_range_at_point(Point::new(point.x, point.y))?;
                Some(Retained::into_super(TextRange::new(self.mtm(), range)))
            })
        }
    }

    // SAFETY: the method below carries UIKit's selector and signature for
    // this protocol and runs on the main thread, where UIKit calls it.
    unsafe impl UITextInteractionDelegate for TextInputView {
        /// The interaction sits on the window's view, so its gestures begin
        /// only over the focused field and on its selection's handles:
        /// anywhere else, a control beside the field included, the touch is
        /// the application's. `point` is in the window's view, whose
        /// coordinates the store's are.
        #[unsafe(method(interactionShouldBegin:atPoint:))]
        fn interaction_should_begin(&self, _interaction: &UITextInteraction, point: CGPoint) -> bool {
            os_entry("FluiTextInputView interactionShouldBegin:atPoint:", (), AfterPanic::Answer(|()| false), || {
                self.begins_at(Point::new(point.x, point.y))
            })
        }

        /// The interaction's own gesture began (a drag of the caret or a
        /// handle, a long press): a move of the field during it is told once,
        /// when it ends.
        #[unsafe(method(interactionWillBegin:))]
        fn interaction_will_begin(&self, _interaction: &UITextInteraction) {
            os_entry("FluiTextInputView interactionWillBegin:", (), AfterPanic::Answer(|()| ()), || {
                self.client().set_interacting(true);
            });
        }

        #[unsafe(method(interactionDidEnd:))]
        fn interaction_did_end(&self, _interaction: &UITextInteraction) {
            os_entry("FluiTextInputView interactionDidEnd:", (), AfterPanic::Answer(|()| ()), || {
                self.client().set_interacting(false);
            });
        }
    }
);

impl TextInputView {
    /// A responder serving the field the window's store host focuses, which
    /// tells `delivery`'s window the Return and Backspace presses.
    pub(super) fn new(mtm: MainThreadMarker, delivery: Weak<WindowDelivery>) -> Retained<Self> {
        let client = TextClient::for_uikit();
        let this = Self::alloc(mtm).set_ivars(ContainedIvars::new(ResponderIvars {
            client: Rc::clone(&client),
            delivery,
            traits: Cell::new(KeyboardTraits::DEFAULT),
            reload_owed: Cell::new(false),
            tokenizer: OnceCell::new(),
            input_delegate: RefCell::new(None),
            interaction: OnceCell::new(),
        }));
        // SAFETY: UIView's designated initializer, on a fresh allocation.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: CGRect::ZERO] };
        let interaction =
            UITextInteraction::textInteractionForMode(UITextInteractionMode::Editable, mtm);
        let responder: &UIResponder = &this;
        // SAFETY: `setTextInput:` requires a responder that implements
        // `UITextInput`, which a `FluiTextInputView` does (`unsafe impl
        // UITextInput` above). The interaction holds it weakly, as it holds
        // its delegate, so the interaction in the view's ivar makes no cycle.
        unsafe { interaction.setTextInput(Some(responder)) };
        interaction.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        this.ivars()
            .interaction
            .set(interaction)
            .expect("BUG: a new view's interaction is set once, here");
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

    /// The scalar before or after `at`, read from the field's store.
    fn extended(&self, at: Utf16Offset, backward: bool) -> Option<Utf16Range> {
        self.client().extended_range(at, backward)
    }

    /// The rectangle of `range`'s first line in this view's points, from the
    /// field's layout; empty where no field has focus or its layout has none.
    fn rect(&self, range: Option<Utf16Range>) -> CGRect {
        range
            .and_then(|range| self.client().first_rect(Some(range)))
            .map_or(CGRect::ZERO, |(bounds, _)| cg_rect(bounds))
    }

    /// A press and release of `key`, told to the window as a hardware key
    /// would be, then a frame asked for.
    fn press(&self, key: NamedKey, code: Code) {
        tracing::debug!(target: LOG, "keyboard key: {key:?}");
        let Some(delivery) = self.ivars().delivery.upgrade() else {
            return;
        };
        for state in [KeyState::Down(KeyPress::First), KeyState::Up] {
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
            self.remove_interaction();
            return;
        };
        let traits = keyboard_traits(status);
        let changed = self.ivars().traits.replace(traits) != traits;
        // Before the view becomes first responder: installing an interaction
        // may clear the text input's `inputDelegate`, which becoming first
        // responder then sets to the keyboard's.
        self.install_interaction();
        if !self.isFirstResponder() {
            self.becomeFirstResponder();
        } else if changed {
            // A reload under marked text would end the composition: it waits
            // for the composition to end ([`Self::reload_if_owed`]).
            if self.client().composes() {
                self.ivars().reload_owed.set(true);
            } else {
                self.ivars().reload_owed.set(false);
                self.reloadInputViews();
            }
        }
    }

    /// Whether UIKit's selection gestures begin at `point`
    /// ([`gesture_begins`]): where a press lands on the focused field by the
    /// window's own hit test, or, where nothing else takes the press, within
    /// a handle's reach of the edge a shown selection handle stands on. The
    /// edges come from the rectangles cut to the part of the field the window
    /// shows, so a handle scrolled or clipped out of view reaches nothing,
    /// and a control over the field or beside a handle keeps its tap.
    fn begins_at(&self, point: Point<f64>) -> bool {
        let client = self.client();
        let Some(target) = client.press_target_at(point) else {
            return false;
        };
        let rects = if target == PressTarget::Nothing {
            client
                .selected_range()
                .filter(|range| !range.is_empty())
                .map(|selection| client.selection_rects(selection))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        gesture_begins(target, point, &rects, HANDLE_REACH)
    }

    /// Put UIKit's text interaction on the window's view, the view the
    /// touches land on, unless it is there already: the view's own frame is
    /// empty, so no touch would reach a gesture on it.
    fn install_interaction(&self) {
        let Some(interaction) = self.ivars().interaction.get() else {
            return;
        };
        if interaction.view().is_some() {
            return;
        }
        if let Some(window_view) = self.superview() {
            window_view.addInteraction(ProtocolObject::from_ref(&**interaction));
        }
    }

    /// Take UIKit's text interaction off the view it is on, if any. A
    /// gesture under way ends with it, though UIKit may never send its
    /// `interactionDidEnd:`.
    fn remove_interaction(&self) {
        let Some(interaction) = self.ivars().interaction.get() else {
            return;
        };
        if let Some(view) = interaction.view() {
            view.removeInteraction(ProtocolObject::from_ref(&**interaction));
        }
        self.client().set_interacting(false);
    }

    /// Reload the keyboard for traits that changed while it composed, once
    /// its marked text has ended.
    fn reload_if_owed(&self) {
        if !self.client().composes() && self.ivars().reload_owed.replace(false) {
            self.reloadInputViews();
        }
    }

    /// Tell the keyboard the document changed under it, which it did not
    /// do: it drops its marked text and reads the field again.
    fn text_changed_under_keyboard(&self) {
        let delegate = self.delegate();
        let input = ProtocolObject::<dyn UITextInput>::from_ref(self);
        if let Some(delegate) = &delegate {
            delegate.textDidChange(Some(input));
        }
    }

    /// Tell UIKit the field itself moved in the window (a scroll, a
    /// transform, the keyboard lifting the view), or another field is
    /// served: `UITextInteraction` keeps the caret, highlight and handles it
    /// drew, and told the selection changed it asks `caretRectForPosition:`
    /// and `selectionRectsForRange:` again (`UITextInputDelegate`'s
    /// `selectionWillChange:` and `selectionDidChange:`, the pair a text input
    /// sends when what UIKit shows of its selection is out of date). The
    /// client sends it only for such a move, never for a caret move, and
    /// never while the keyboard holds marked text or the interaction's own
    /// gesture is under way: a keyboard told the selection changed mid
    /// composition may end it.
    fn geometry_changed_under_keyboard(&self) {
        let delegate = self.delegate();
        let input = ProtocolObject::<dyn UITextInput>::from_ref(self);
        if let Some(delegate) = &delegate {
            delegate.selectionWillChange(Some(input));
            delegate.selectionDidChange(Some(input));
        }
    }

    /// Focus moved off the field an edit menu may be open over: the menu is
    /// dismissed, and the interaction and the keyboard's input delegate stay
    /// as they are. iOS 16 and later present the text interaction's edit
    /// menu through a `UIEditMenuInteraction` on its view, which
    /// `dismissMenu` closes; before iOS 16 the menu is `UIMenuController`'s,
    /// which `hideMenu` (iOS 13) closes.
    fn dismiss_edit_menu(&self) {
        if objc2::available!(ios = 16.0) {
            let views = [
                self.ivars()
                    .interaction
                    .get()
                    .and_then(|interaction| interaction.view()),
                Some(Retained::into_super(self.retain())),
            ];
            for view in views.iter().flatten() {
                for interaction in view.interactions().iter() {
                    let object: &AnyObject = interaction.as_ref();
                    if let Some(menu) = object.downcast_ref::<UIEditMenuInteraction>() {
                        menu.dismissMenu();
                    }
                }
            }
        } else {
            #[expect(
                deprecated,
                reason = "UIMenuController is the edit menu below iOS 16, the floor being 13"
            )]
            objc2_ui_kit::UIMenuController::sharedMenuController(self.mtm()).hideMenu();
        }
    }

    /// Tell the keyboard the selection moved under it.
    fn selection_changed_under_keyboard(&self) {
        let delegate = self.delegate();
        let input = ProtocolObject::<dyn UITextInput>::from_ref(self);
        if let Some(delegate) = &delegate {
            delegate.selectionDidChange(Some(input));
        }
    }

    /// The previous published document is still readable during this notice.
    fn document_will_change(&self, change: DocumentChange) {
        let delegate = self.delegate();
        let input = ProtocolObject::<dyn UITextInput>::from_ref(self);
        if let Some(delegate) = delegate {
            match change {
                DocumentChange::Text | DocumentChange::Composition => {
                    delegate.textWillChange(Some(input));
                }
                DocumentChange::Selection => delegate.selectionWillChange(Some(input)),
            }
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
    /// bar and keeps no rectangle to invalidate: a caret or composition move
    /// is told nothing, so the keyboard is never told the selection changed
    /// while it composes.
    fn invalidate_character_coordinates(&self) {}

    /// The field itself moved, or another is served: UIKit's text
    /// interaction asks for the caret and selection rectangles again
    /// ([`TextInputView::geometry_changed_under_keyboard`]).
    fn geometry_changed(&self) {
        if let Some(view) = self.view.load() {
            view.geometry_changed_under_keyboard();
        }
    }

    fn field_changed(&self, status: Option<TextStoreStatus>) {
        if let Some(view) = self.view.load() {
            view.serve(status);
        }
    }

    /// An edit menu open over the field focus left is dismissed; the
    /// interaction and the input delegate stay.
    fn field_left(&self) {
        if let Some(view) = self.view.load() {
            view.dismiss_edit_menu();
        }
    }

    fn document_changed(&self, change: DocumentChange, _marked_text: MarkedText) {
        if let Some(view) = self.view.load() {
            match change {
                DocumentChange::Text | DocumentChange::Composition => {
                    view.text_changed_under_keyboard();
                }
                DocumentChange::Selection => view.selection_changed_under_keyboard(),
            }
        }
    }

    fn document_will_change(&self, change: DocumentChange) {
        if let Some(view) = self.view.load() {
            view.document_will_change(change);
        }
    }
}

/// `bounds`, in the window's logical coordinates, in the responder's points,
/// which are the same.
fn cg_rect(bounds: Bounds<f64>) -> CGRect {
    let Bounds { origin, size } = bounds;
    CGRect::new(
        CGPoint::new(origin.x, origin.y),
        CGSize::new(size.width, size.height),
    )
}

/// The edit command an edit-menu action asks the field for; `None` for an
/// action that is not one of the field's.
fn edit_command(action: Sel) -> Option<EditCommand> {
    [
        (sel!(cut:), EditCommand::Cut),
        (sel!(copy:), EditCommand::Copy),
        (sel!(paste:), EditCommand::Paste),
        (sel!(select:), EditCommand::SelectWord),
        (sel!(selectAll:), EditCommand::SelectAll),
    ]
    .into_iter()
    .find_map(|(selector, command)| (selector == action).then_some(command))
}

/// Whether the general pasteboard holds text. `hasStrings` answers from the
/// pasteboard's types without reading an item, which UIKit does not count as
/// a paste: it shows no paste prompt or banner, as reading `string` would.
fn pasteboard_has_text() -> bool {
    let pasteboard = UIPasteboard::generalPasteboard();
    // SAFETY: a getter of the general pasteboard on the main thread, which
    // `canPerformAction:withSender:` runs on; it takes no argument.
    unsafe { pasteboard.hasStrings() }
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
