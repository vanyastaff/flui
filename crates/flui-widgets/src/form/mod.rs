//! [`Form`], [`FormField`] and [`RawTextFormField`] — a group of fields that
//! validate, save and reset together.
//!
//! Flutter parity: `widgets/form.dart` (tag `3.44.0`): `Form`, `FormState`,
//! `FormField`, `FormFieldState` and `AutovalidateMode`. The behaviour is
//! Flutter's; the shape is Rust's:
//!
//! - **Handles, not `GlobalKey<FormState>`.** A [`FormHandle`] or
//!   [`FormFieldHandle`] the caller creates (or [`Form::of`]) is the
//!   imperative surface — `validate`, `save`, `reset`, the field's value and
//!   error.
//! - **Validation runs at the event.** Flutter validates inside `build`; FLUI's
//!   `build(&self)` cannot mutate, so a field validates when its value changes,
//!   when it mounts or is reconfigured, when it loses focus, or when the form
//!   asks — the same moments, since each of those is what schedules Flutter's
//!   build. The field then schedules its own rebuild.
//! - **Registration in lifecycle hooks.** A field registers with its form in
//!   `init_state`, moves in `did_change_dependencies`, and leaves in
//!   `dispose`; Flutter registers in `build` and leaves in `deactivate`.
//!
//! `flui-widgets/ARCHITECTURE.md`'s `## Mapping decisions` records each
//! divergence and the test that pins it.
//!
//! # Not ported
//!
//! Pop veto (`canPop`/`onPopInvokedWithResult`), restoration,
//! `validateGranularly`, `FormField.errorBuilder`, and the announcement
//! `validate()` makes through `SemanticsService` (the error line is a live
//! region instead).

mod form_field;
mod raw_text_form_field;
pub(crate) mod text_form_field_core;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_rendering::semantics::SemanticsRole;
use flui_view::element::ElementKind;
use flui_view::impl_inherited_view;
use flui_view::prelude::*;

pub use form_field::{
    FormField, FormFieldHandle, FormFieldSetter, FormFieldState, FormFieldValidator,
};
pub use raw_text_form_field::{RawTextFormField, RawTextFormFieldState};

use crate::semantics::Semantics;

/// When a field validates without an explicit `validate()` — Flutter's
/// `AutovalidateMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum AutovalidateMode {
    /// Only when `validate()` is called.
    #[default]
    Disabled,
    /// At mount and on every change.
    Always,
    /// On every change after the user's first edit.
    OnUserInteraction,
    /// On a change after the user's first edit, while an error is shown —
    /// so a correction clears the error, but no error appears unasked.
    OnUserInteractionIfError,
    /// When the field loses focus.
    OnUnfocus,
}

/// What a [`Form`] needs from each registered field, whatever its value type.
pub(crate) trait FormFieldEntry {
    /// Run the validator, show its result, and report whether it passed.
    fn validate(&self) -> bool;
    /// Hand the current value to `on_saved`.
    fn save(&self);
    /// Back to the initial value, errors and interaction cleared.
    fn reset(&self);
    fn has_interacted_by_user(&self) -> bool;
    fn has_error(&self) -> bool;
}

/// The form's shared state; fields hold it weakly.
#[derive(Default)]
pub(crate) struct FormInner {
    /// Registered fields, in registration order (Flutter's insertion-ordered
    /// `Set<FormFieldState>`).
    fields: RefCell<Vec<Rc<dyn FormFieldEntry>>>,
    interacted: Cell<bool>,
    mode: Cell<AutovalidateMode>,
    on_changed: RefCell<Option<Rc<dyn Fn()>>>,
    /// Set while `reset` visits its fields: a field's change notice then
    /// reports `on_changed` but defers autovalidation to the end, as
    /// Flutter's single rebuild after the loop does.
    resetting: Cell<bool>,
}

/// The form's imperative surface — Flutter's `FormState`, reached by a handle
/// the caller creates and passes to [`Form::handle`], or from [`Form::of`],
/// instead of a `GlobalKey<FormState>`.
///
/// Cheap to clone; every clone names the same form. Owner-thread only.
///
/// [`Self::has_interacted_by_user`] reads plain state, not a signal, so a
/// `build` that calls it does not subscribe; see [`FormFieldHandle`]'s
/// "Reads are not tracked".
#[derive(Clone, Default)]
pub struct FormHandle {
    inner: Rc<FormInner>,
}

impl std::fmt::Debug for FormHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormHandle")
            .field("fields", &self.inner.fields.borrow().len())
            .field("interacted", &self.inner.interacted.get())
            .field("mode", &self.inner.mode.get())
            .finish_non_exhaustive()
    }
}

impl FormHandle {
    /// A handle for a form not mounted yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate every registered field and show each result; mark the form
    /// interacted. `true` iff every field passed. Flutter's
    /// `FormState.validate`.
    pub fn validate(&self) -> bool {
        self.inner.interacted.set(true);
        self.validate_fields()
    }

    /// Call every field's `on_saved` with its current value, in registration
    /// order — Flutter's `FormState.save`.
    pub fn save(&self) {
        for field in self.fields() {
            field.save();
        }
    }

    /// Every field back to its initial value, its error and interaction
    /// cleared; then the form's interaction is cleared and `on_changed`
    /// runs — Flutter's `FormState.reset`.
    pub fn reset(&self) {
        self.inner.resetting.set(true);
        for field in self.fields() {
            field.reset();
        }
        self.inner.resetting.set(false);
        self.inner.interacted.set(false);
        self.field_did_change();
    }

    /// Whether the user has edited any field, or `validate()` ran since the
    /// last reset.
    #[must_use]
    pub fn has_interacted_by_user(&self) -> bool {
        self.inner.interacted.get()
    }

    /// The registered fields, cloned out so no borrow is held while a
    /// field's callbacks run.
    fn fields(&self) -> Vec<Rc<dyn FormFieldEntry>> {
        self.inner.fields.borrow().clone()
    }

    fn validate_fields(&self) -> bool {
        // Every field, not the first failure: each shows its own error.
        let mut valid = true;
        for field in self.fields() {
            valid &= field.validate();
        }
        valid
    }

    pub(crate) fn register(&self, field: Rc<dyn FormFieldEntry>) {
        self.inner.fields.borrow_mut().push(field);
    }

    /// Put `new` in `old`'s place, keeping the registration order; `new`
    /// joins at the end when `old` is not registered.
    pub(crate) fn replace(&self, old: &Rc<dyn FormFieldEntry>, new: Rc<dyn FormFieldEntry>) {
        let mut fields = self.inner.fields.borrow_mut();
        match fields
            .iter_mut()
            .find(|registered| Rc::ptr_eq(registered, old))
        {
            Some(slot) => *slot = new,
            None => fields.push(new),
        }
    }

    pub(crate) fn unregister(&self, field: &Rc<dyn FormFieldEntry>) {
        self.inner
            .fields
            .borrow_mut()
            .retain(|registered| !Rc::ptr_eq(registered, field));
    }

    pub(crate) fn mode(&self) -> AutovalidateMode {
        self.inner.mode.get()
    }

    pub(crate) fn downgrade(&self) -> Weak<FormInner> {
        Rc::downgrade(&self.inner)
    }

    pub(crate) fn upgrade(inner: &Weak<FormInner>) -> Option<Self> {
        inner.upgrade().map(|inner| Self { inner })
    }

    pub(crate) fn same_form(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    /// A field's value changed, or it was reset — Flutter's `_fieldDidChange`
    /// followed by the form's autovalidation in `FormState.build`.
    pub(crate) fn field_did_change(&self) {
        let on_changed = self.inner.on_changed.borrow().clone();
        if let Some(on_changed) = on_changed {
            on_changed();
        }
        if self.inner.resetting.get() {
            return;
        }
        let fields = self.fields();
        self.inner
            .interacted
            .set(fields.iter().any(|field| field.has_interacted_by_user()));
        self.autovalidate(&fields);
    }

    /// The form-level autovalidation: every field, when the form's mode asks.
    fn autovalidate(&self, fields: &[Rc<dyn FormFieldEntry>]) {
        let interacted = self.inner.interacted.get();
        let validate = match self.inner.mode.get() {
            AutovalidateMode::Always => true,
            AutovalidateMode::OnUserInteraction => interacted,
            AutovalidateMode::OnUserInteractionIfError => {
                interacted && fields.iter().any(|field| field.has_error())
            }
            AutovalidateMode::Disabled | AutovalidateMode::OnUnfocus => false,
        };
        if validate {
            for field in fields {
                field.validate();
            }
        }
    }
}

// ============================================================================
// Form
// ============================================================================

/// A group of form fields that validate, save and reset together — Flutter's
/// `Form`.
///
/// Reach the form through a [`FormHandle`] passed to [`Self::handle`], or
/// from a descendant with [`Form::of`]. The form is a semantics node with the
/// form role.
#[derive(Clone)]
pub struct Form {
    child: BoxedView,
    handle: Option<FormHandle>,
    autovalidate_mode: AutovalidateMode,
    on_changed: Option<Rc<dyn Fn()>>,
}

impl std::fmt::Debug for Form {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Form")
            .field("handle", &self.handle)
            .field("autovalidate_mode", &self.autovalidate_mode)
            .field("on_changed", &self.on_changed.is_some())
            .finish_non_exhaustive()
    }
}

impl Form {
    /// A form around `child`.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
            handle: None,
            autovalidate_mode: AutovalidateMode::Disabled,
            on_changed: None,
        }
    }

    /// Drive the form through `handle`. Without one the form keeps its own,
    /// reachable through [`Form::of`].
    #[must_use]
    pub fn handle(mut self, handle: FormHandle) -> Self {
        self.handle = Some(handle);
        self
    }

    /// When every field validates without an explicit `validate()`.
    #[must_use]
    pub fn autovalidate_mode(mut self, mode: AutovalidateMode) -> Self {
        self.autovalidate_mode = mode;
        self
    }

    /// Called when any field's value changes, and on `reset`.
    #[must_use]
    pub fn on_changed(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_changed = Some(Rc::new(callback));
        self
    }

    /// The enclosing form's handle — Flutter's `Form.of`.
    ///
    /// # Panics
    ///
    /// Outside a `Form`. [`Form::maybe_of`] does not.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> FormHandle {
        Self::maybe_of(ctx).expect(
            "Form::of() was called with a context that does not contain a Form; wrap the \
             fields in a Form, or use Form::maybe_of",
        )
    }

    /// The enclosing form's handle, if any, registering a dependency on it —
    /// Flutter's `Form.maybeOf`.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<FormHandle> {
        ctx.depend_on::<FormScope, _>(|scope| scope.handle.clone())
    }
}

impl View for Form {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

/// The state behind [`Form`]: its handle and configuration.
pub struct FormState {
    handle: FormHandle,
}

impl std::fmt::Debug for FormState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormState")
            .field("handle", &self.handle)
            .finish()
    }
}

impl FormState {
    fn configure(&self, view: &Form) {
        self.handle.inner.mode.set(view.autovalidate_mode);
        self.handle
            .inner
            .on_changed
            .borrow_mut()
            .clone_from(&view.on_changed);
    }
}

impl StatefulView for Form {
    type State = FormState;

    fn create_state(&self) -> FormState {
        let state = FormState {
            handle: self.handle.clone().unwrap_or_default(),
        };
        state.configure(self);
        state
    }
}

impl ViewState<Form> for FormState {
    fn did_update_view(&mut self, old_view: &Form, new_view: &Form) {
        let handle_changed = match (&old_view.handle, &new_view.handle) {
            (Some(old), Some(new)) => !old.same_form(new),
            (None, None) => false,
            _ => true,
        };
        if handle_changed {
            // The scope notifies its dependents, and each field registers
            // with the new form in `did_change_dependencies`.
            self.handle = new_view.handle.clone().unwrap_or_default();
        }
        self.configure(new_view);
        // A reconfigured form rebuilds in Flutter, and its build runs the
        // form-level autovalidation.
        let fields = self.handle.fields();
        self.handle.autovalidate(&fields);
    }

    fn build(&self, view: &Form, _ctx: &dyn BuildContext) -> impl IntoView {
        Semantics::new()
            .container(true)
            .role(SemanticsRole::Form)
            .child(FormScope {
                handle: self.handle.clone(),
                child: view.child.clone(),
            })
    }
}

/// Publishes the form's handle to its fields.
#[derive(Clone)]
struct FormScope {
    handle: FormHandle,
    child: BoxedView,
}

impl std::fmt::Debug for FormScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormScope")
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

impl InheritedView for FormScope {
    type Data = FormHandle;

    fn data(&self) -> &Self::Data {
        &self.handle
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !self.handle.same_form(&old.handle)
    }
}

impl_inherited_view!(FormScope);
