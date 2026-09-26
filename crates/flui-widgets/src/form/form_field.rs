//! [`FormField`] and [`FormFieldHandle`] — one validated value in a [`Form`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{RebuildHandle, RebuildReason};

use super::{AutovalidateMode, Form, FormFieldEntry, FormHandle, FormInner};
use crate::interaction::Focus;

/// Checks a value; `Some(message)` is the error to show — Flutter's
/// `FormFieldValidator<T>`.
pub type FormFieldValidator<T> = Rc<dyn Fn(&T) -> Option<String>>;
/// Receives the value on `save` — Flutter's `FormFieldSetter<T>`.
pub type FormFieldSetter<T> = Rc<dyn Fn(&T)>;
/// Builds the field's content from its handle — Flutter's
/// `FormFieldBuilder<T>`, which receives the `FormFieldState`.
pub type FormFieldBuilder<T> = Rc<dyn Fn(&dyn BuildContext, &FormFieldHandle<T>) -> BoxedView>;

/// The field's state, shared by its handle, its widget state and its form.
struct FieldInner<T> {
    /// `None` until the field mounts.
    value: RefCell<Option<T>>,
    initial: RefCell<Option<T>>,
    error: RefCell<Option<String>>,
    interacted: Cell<bool>,
    validator: RefCell<Option<FormFieldValidator<T>>>,
    on_saved: RefCell<Option<FormFieldSetter<T>>>,
    on_reset: RefCell<Option<Rc<dyn Fn()>>>,
    enabled: Cell<bool>,
    mode: Cell<AutovalidateMode>,
    force_error_text: RefCell<Option<String>>,
    /// The mounted field's rebuild, taken in `init_state`.
    rebuild: RefCell<Option<RebuildHandle>>,
    /// The form this field is registered with.
    form: RefCell<Weak<FormInner>>,
    /// Where a value change is pushed back to — a text field's controller.
    value_sink: RefCell<Option<ValueSink<T>>>,
    /// Where the value is read from before it is used, when something other
    /// than the user's edits can change it — a text field's controller.
    value_source: RefCell<Option<ValueSource<T>>>,
}

/// Receives each value the field is set to.
type ValueSink<T> = Rc<dyn Fn(&T)>;
/// Supplies the field's current value.
type ValueSource<T> = Rc<dyn Fn() -> T>;

impl<T> Default for FieldInner<T> {
    fn default() -> Self {
        Self {
            value: RefCell::new(None),
            initial: RefCell::new(None),
            error: RefCell::new(None),
            interacted: Cell::new(false),
            validator: RefCell::new(None),
            on_saved: RefCell::new(None),
            on_reset: RefCell::new(None),
            enabled: Cell::new(true),
            mode: Cell::new(AutovalidateMode::Disabled),
            force_error_text: RefCell::new(None),
            rebuild: RefCell::new(None),
            form: RefCell::new(Weak::new()),
            value_sink: RefCell::new(None),
            value_source: RefCell::new(None),
        }
    }
}

impl<T: Clone + 'static> FieldInner<T> {
    fn form(&self) -> Option<FormHandle> {
        FormHandle::upgrade(&self.form.borrow())
    }

    fn schedule_rebuild(&self) {
        let rebuild = self.rebuild.borrow().clone();
        if let Some(rebuild) = rebuild {
            rebuild.schedule(RebuildReason::StateChange);
        }
    }

    /// Adopt the source's value, when the field has one, without counting
    /// it as the user's edit.
    fn sync_from_source(&self) {
        let source = self.value_source.borrow().clone();
        if let Some(source) = source {
            let value = source();
            *self.value.borrow_mut() = Some(value);
        }
    }

    fn current(&self) -> Option<T> {
        self.sync_from_source();
        self.value.borrow().clone()
    }

    /// The validator's verdict on the current value, `force_error_text`
    /// first — without showing it. Runs with no borrow held.
    fn verdict(&self) -> Option<String> {
        let forced = self.force_error_text.borrow().clone();
        if forced.is_some() {
            return forced;
        }
        let validator = self.validator.borrow().clone();
        let value = self.current();
        match (validator, value) {
            (Some(validator), Some(value)) => validator(&value),
            _ => None,
        }
    }

    /// Validate and show the result — Flutter's `_validate` inside a
    /// `setState`. Rebuilds only when the shown error changed.
    fn run_validate(&self) {
        let error = self.verdict();
        let changed = *self.error.borrow() != error;
        *self.error.borrow_mut() = error;
        if changed {
            self.schedule_rebuild();
        }
    }

    /// The field-level autovalidation of Flutter's `FormFieldState.build`.
    /// The field's own mode only: it does not fall back to the form's.
    fn autovalidate(&self) {
        if !self.enabled.get() {
            return;
        }
        let validate = match self.mode.get() {
            AutovalidateMode::Always => true,
            AutovalidateMode::OnUserInteraction => self.interacted.get(),
            AutovalidateMode::OnUserInteractionIfError => {
                self.interacted.get() && self.error.borrow().is_some()
            }
            AutovalidateMode::Disabled | AutovalidateMode::OnUnfocus => false,
        };
        if validate {
            self.run_validate();
        }
    }

    /// Whether losing focus validates: the field's mode is `OnUnfocus`, or
    /// the form's is and the field's is not `Always` (Flutter's
    /// `FormFieldState.build`).
    fn validates_on_unfocus(&self) -> bool {
        let own = self.mode.get();
        own == AutovalidateMode::OnUnfocus
            || (own != AutovalidateMode::Always
                && self
                    .form()
                    .is_some_and(|form| form.mode() == AutovalidateMode::OnUnfocus))
    }

    fn push_to_sink(&self, value: &T) {
        let sink = self.value_sink.borrow().clone();
        if let Some(sink) = sink {
            sink(value);
        }
    }

    fn did_change(&self, value: T) {
        *self.value.borrow_mut() = Some(value.clone());
        self.interacted.set(true);
        self.push_to_sink(&value);
        self.autovalidate();
        self.schedule_rebuild();
        if let Some(form) = self.form() {
            form.field_did_change();
        }
    }

    fn reset(&self) {
        let initial = self.initial.borrow().clone();
        self.value.borrow_mut().clone_from(&initial);
        self.interacted.set(false);
        *self.error.borrow_mut() = None;
        if let Some(initial) = &initial {
            self.push_to_sink(initial);
        }
        let on_reset = self.on_reset.borrow().clone();
        if let Some(on_reset) = on_reset {
            on_reset();
        }
        self.schedule_rebuild();
        if let Some(form) = self.form() {
            form.field_did_change();
        }
    }
}

impl<T: Clone + 'static> FormFieldEntry for FieldInner<T> {
    fn validate(&self) -> bool {
        self.run_validate();
        self.error.borrow().is_none()
    }

    fn save(&self) {
        let on_saved = self.on_saved.borrow().clone();
        if let (Some(on_saved), Some(value)) = (on_saved, self.current()) {
            on_saved(&value);
        }
    }

    fn reset(&self) {
        Self::reset(self);
    }

    fn has_interacted_by_user(&self) -> bool {
        self.interacted.get()
    }

    fn has_error(&self) -> bool {
        self.error.borrow().is_some()
    }
}

// ============================================================================
// FormFieldHandle
// ============================================================================

/// One field's imperative surface — Flutter's `FormFieldState<T>`, reached
/// by a handle the caller creates and passes to [`FormField::handle`], and
/// handed to the field's builder.
///
/// Cheap to clone; every clone names the same field. Owner-thread only.
///
/// A mounted field given a different handle on a later rebuild moves onto
/// it: the new handle takes the field's value, error, interaction and place
/// in the form, and the old one is detached — it keeps its last value but no
/// longer reaches the field.
///
/// # Reads are not tracked
///
/// The getters ([`Self::value`], [`Self::error_text`], [`Self::has_error`],
/// [`Self::is_valid`], [`Self::has_interacted_by_user`]) read plain state,
/// not a signal, so a `build` that calls them does not subscribe. The field
/// rebuilds its own content when its error changes; any other widget that
/// shows a field's state — a submit button enabled by [`Self::is_valid`] —
/// is rebuilt by the caller, for example from [`Form::on_changed`]. The
/// getters are meant for event handlers and the field's own builder.
/// Flutter's `FormFieldState` is not listenable either.
pub struct FormFieldHandle<T> {
    inner: Rc<FieldInner<T>>,
}

impl<T> Clone for FormFieldHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> Default for FormFieldHandle<T> {
    fn default() -> Self {
        Self {
            inner: Rc::new(FieldInner::default()),
        }
    }
}

impl<T> std::fmt::Debug for FormFieldHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormFieldHandle")
            .field("mounted", &self.inner.value.borrow().is_some())
            .field("error", &self.inner.error.borrow())
            .field("interacted", &self.inner.interacted.get())
            .finish_non_exhaustive()
    }
}

impl<T: Clone + 'static> FormFieldHandle<T> {
    /// A handle for a field not mounted yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The field's current value.
    ///
    /// # Panics
    ///
    /// Before the field mounts.
    #[must_use]
    pub fn value(&self) -> T {
        self.inner
            .current()
            .expect("FormFieldHandle::value() was called before its FormField mounted")
    }

    /// The error the field shows, if any.
    #[must_use]
    pub fn error_text(&self) -> Option<String> {
        self.inner.error.borrow().clone()
    }

    /// Whether the field shows an error.
    #[must_use]
    pub fn has_error(&self) -> bool {
        self.inner.error.borrow().is_some()
    }

    /// Whether the value passes validation, without showing the result —
    /// Flutter's `FormFieldState.isValid`.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.inner.verdict().is_none()
    }

    /// Whether the user has edited the field since it mounted or was reset.
    #[must_use]
    pub fn has_interacted_by_user(&self) -> bool {
        self.inner.interacted.get()
    }

    /// A user edit: set the value, mark the field interacted, run the
    /// field's and the form's autovalidation, and report the change to the
    /// form — Flutter's `FormFieldState.didChange`.
    pub fn did_change(&self, value: T) {
        self.inner.did_change(value);
    }

    /// Set the value without marking interaction, validating or rebuilding
    /// — Flutter's `FormFieldState.setValue`.
    ///
    /// A text form field's value is its controller's text, so there this
    /// writes the text into the controller too; Flutter's `setValue` leaves
    /// the controller alone and the two disagree until the next edit.
    pub fn set_value(&self, value: T) {
        *self.inner.value.borrow_mut() = Some(value.clone());
        self.inner.push_to_sink(&value);
    }

    /// Validate, show the result, and report whether it passed — Flutter's
    /// `FormFieldState.validate`.
    pub fn validate(&self) -> bool {
        FormFieldEntry::validate(&*self.inner)
    }

    /// Back to the initial value, error and interaction cleared, then
    /// `on_reset` — Flutter's `FormFieldState.reset`.
    pub fn reset(&self) {
        self.inner.reset();
    }

    /// Push every later value change into `sink` and read the value from
    /// `source` before it is used — how a text field keeps its controller
    /// and its value one.
    pub(crate) fn bind(&self, sink: ValueSink<T>, source: ValueSource<T>) {
        *self.inner.value_sink.borrow_mut() = Some(sink);
        *self.inner.value_source.borrow_mut() = Some(source);
    }

    /// Whether `self` and `other` name the same field.
    pub(crate) fn same_field(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    fn as_entry(&self) -> Rc<dyn FormFieldEntry> {
        Rc::clone(&self.inner) as Rc<dyn FormFieldEntry>
    }
}

// ============================================================================
// FormField
// ============================================================================

/// One validated value in a [`Form`] — Flutter's `FormField<T>`.
///
/// The builder receives the field's [`FormFieldHandle`] and reads the value
/// and error from it; an edit reports back through
/// [`FormFieldHandle::did_change`].
pub struct FormField<T> {
    initial_value: T,
    builder: FormFieldBuilder<T>,
    pub(super) validator: Option<FormFieldValidator<T>>,
    pub(super) on_saved: Option<FormFieldSetter<T>>,
    pub(super) on_reset: Option<Rc<dyn Fn()>>,
    enabled: bool,
    autovalidate_mode: AutovalidateMode,
    pub(super) force_error_text: Option<String>,
    handle: Option<FormFieldHandle<T>>,
}

impl<T: Clone> Clone for FormField<T> {
    fn clone(&self) -> Self {
        Self {
            initial_value: self.initial_value.clone(),
            builder: Rc::clone(&self.builder),
            validator: self.validator.clone(),
            on_saved: self.on_saved.clone(),
            on_reset: self.on_reset.clone(),
            enabled: self.enabled,
            autovalidate_mode: self.autovalidate_mode,
            force_error_text: self.force_error_text.clone(),
            handle: self.handle.clone(),
        }
    }
}

impl<T> std::fmt::Debug for FormField<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormField")
            .field("validator", &self.validator.is_some())
            .field("enabled", &self.enabled)
            .field("autovalidate_mode", &self.autovalidate_mode)
            .field("force_error_text", &self.force_error_text)
            .finish_non_exhaustive()
    }
}

impl<T: Clone + 'static> FormField<T> {
    /// A field starting at `initial_value`, its content built by `builder`.
    #[must_use]
    pub fn new(
        initial_value: T,
        builder: impl Fn(&dyn BuildContext, &FormFieldHandle<T>) -> BoxedView + 'static,
    ) -> Self {
        Self {
            initial_value,
            builder: Rc::new(builder),
            validator: None,
            on_saved: None,
            on_reset: None,
            enabled: true,
            autovalidate_mode: AutovalidateMode::Disabled,
            force_error_text: None,
            handle: None,
        }
    }

    /// Check the value; `Some(message)` is the error to show.
    #[must_use]
    pub fn validator(mut self, validator: impl Fn(&T) -> Option<String> + 'static) -> Self {
        self.validator = Some(Rc::new(validator));
        self
    }

    /// Receive the value when the form saves.
    #[must_use]
    pub fn on_saved(mut self, on_saved: impl Fn(&T) + 'static) -> Self {
        self.on_saved = Some(Rc::new(on_saved));
        self
    }

    /// Called after the field resets.
    #[must_use]
    pub fn on_reset(mut self, on_reset: impl Fn() + 'static) -> Self {
        self.on_reset = Some(Rc::new(on_reset));
        self
    }

    /// Whether the field autovalidates (default `true`); an explicit
    /// `validate()` still runs.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// When the field validates on its own — the field's mode only; it does
    /// not fall back to the form's.
    #[must_use]
    pub fn autovalidate_mode(mut self, mode: AutovalidateMode) -> Self {
        self.autovalidate_mode = mode;
        self
    }

    /// Show `text` as the error, whatever the validator says.
    #[must_use]
    pub fn force_error_text(mut self, text: impl Into<String>) -> Self {
        self.force_error_text = Some(text.into());
        self
    }

    /// Drive the field through `handle`. A different handle on a later
    /// rebuild takes the mounted field over (see [`FormFieldHandle`]).
    #[must_use]
    pub fn handle(mut self, handle: FormFieldHandle<T>) -> Self {
        self.handle = Some(handle);
        self
    }
}

impl<T: Clone + 'static> View for FormField<T> {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

/// The state behind [`FormField`].
pub struct FormFieldState<T> {
    handle: FormFieldHandle<T>,
    form: Option<FormHandle>,
}

impl<T> std::fmt::Debug for FormFieldState<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormFieldState")
            .field("handle", &self.handle)
            .field("registered", &self.form.is_some())
            .finish()
    }
}

impl<T: Clone + 'static> FormFieldState<T> {
    fn configure(&self, view: &FormField<T>) {
        let inner = &self.handle.inner;
        inner.validator.borrow_mut().clone_from(&view.validator);
        inner.on_saved.borrow_mut().clone_from(&view.on_saved);
        inner.on_reset.borrow_mut().clone_from(&view.on_reset);
        inner.enabled.set(view.enabled);
        inner.mode.set(view.autovalidate_mode);
        inner
            .force_error_text
            .borrow_mut()
            .clone_from(&view.force_error_text);
        *inner.initial.borrow_mut() = Some(view.initial_value.clone());
    }

    /// Move the mounted field onto `handle`: the field's state moves across,
    /// the new handle takes the old one's place in the form, and the old
    /// handle is detached — it keeps its last value, error and interaction
    /// but no longer rebuilds the field, writes its controller or belongs to
    /// the form.
    fn adopt(&mut self, handle: FormFieldHandle<T>) {
        let old = std::mem::replace(&mut self.handle, handle);
        let (from, to) = (&old.inner, &self.handle.inner);
        to.value.borrow_mut().clone_from(&from.value.borrow());
        to.error.borrow_mut().clone_from(&from.error.borrow());
        to.interacted.set(from.interacted.get());
        *to.rebuild.borrow_mut() = from.rebuild.borrow_mut().take();
        // A text field binds its controller to the new handle itself; a
        // plain field's value binding, if any, moves across.
        let sink = from.value_sink.borrow_mut().take();
        if to.value_sink.borrow().is_none() {
            *to.value_sink.borrow_mut() = sink;
        }
        let source = from.value_source.borrow_mut().take();
        if to.value_source.borrow().is_none() {
            *to.value_source.borrow_mut() = source;
        }
        *to.form.borrow_mut() = std::mem::take(&mut *from.form.borrow_mut());
        if let Some(form) = &self.form {
            form.replace(&old.as_entry(), self.handle.as_entry());
        }
    }

    /// Move this field's registration to the form enclosing it now.
    fn register_with(&mut self, form: Option<FormHandle>) {
        let unchanged = match (&self.form, &form) {
            (Some(held), Some(new)) => held.same_form(new),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        let entry = self.handle.as_entry();
        if let Some(old) = self.form.take() {
            old.unregister(&entry);
        }
        *self.handle.inner.form.borrow_mut() =
            form.as_ref().map_or_else(Weak::new, FormHandle::downgrade);
        if let Some(form) = &form {
            form.register(entry);
            // A field joining a form whose mode is `Always` validates, as the
            // form's next build would.
            if form.mode() == AutovalidateMode::Always {
                self.handle.inner.run_validate();
            }
        }
        self.form = form;
    }
}

impl<T: Clone + 'static> StatefulView for FormField<T> {
    type State = FormFieldState<T>;

    fn create_state(&self) -> FormFieldState<T> {
        let state = FormFieldState {
            handle: self.handle.clone().unwrap_or_default(),
            form: None,
        };
        state.configure(self);
        *state.handle.inner.value.borrow_mut() = Some(self.initial_value.clone());
        state
            .handle
            .inner
            .error
            .borrow_mut()
            .clone_from(&self.force_error_text);
        state
    }
}

impl<T: Clone + 'static> ViewState<FormField<T>> for FormFieldState<T> {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.handle.inner.rebuild.borrow_mut() = Some(ctx.rebuild_handle());
        self.register_with(Form::maybe_of(ctx));
        self.handle.inner.autovalidate();
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.register_with(Form::maybe_of(ctx));
    }

    fn did_update_view(&mut self, old_view: &FormField<T>, new_view: &FormField<T>) {
        if let Some(handle) = &new_view.handle
            && !handle.same_field(&self.handle)
        {
            self.adopt(handle.clone());
        }
        self.configure(new_view);
        if old_view.force_error_text != new_view.force_error_text {
            // A forced error shows as given; withdrawing it re-runs the
            // validator only when the field's own mode would.
            self.handle
                .inner
                .error
                .borrow_mut()
                .clone_from(&new_view.force_error_text);
        }
        // Flutter's field rebuilds on a new configuration, and its build
        // runs the field-level autovalidation.
        self.handle.inner.autovalidate();
    }

    fn dispose(&mut self) {
        if let Some(form) = self.form.take() {
            form.unregister(&self.handle.as_entry());
        }
        *self.handle.inner.form.borrow_mut() = Weak::new();
        *self.handle.inner.rebuild.borrow_mut() = None;
    }

    fn build(&self, view: &FormField<T>, ctx: &dyn BuildContext) -> impl IntoView {
        let content = (view.builder)(ctx, &self.handle);
        // Always wrapped, so a mode change never remounts the content; the
        // wrapper validates on focus loss only when the modes ask for it.
        // Not focusable and skipped by traversal, so Tab moves between the
        // fields inside, never onto the wrapper.
        let inner = Rc::clone(&self.handle.inner);
        Focus::new(content)
            .can_request_focus(false)
            .skip_traversal(true)
            .include_semantics(false)
            .debug_label("FormField")
            .on_focus_change(move |focused| {
                if !focused && inner.validates_on_unfocus() {
                    inner.run_validate();
                }
            })
    }
}
