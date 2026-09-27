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
use flui_view::EventContextError;
use flui_view::element::ElementKind;
use flui_view::impl_inherited_view;
use flui_view::prelude::*;

pub use form_field::{
    FormField, FormFieldHandle, FormFieldHandleAlreadyAttached, FormFieldSetter, FormFieldState,
    FormFieldValidator,
};
pub use raw_text_form_field::{RawTextFormField, RawTextFormFieldState};

use crate::semantics::Semantics;
use crate::support::{EventCallback, event_callback};

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
    fn check_context(&self, cx: &EventCx<'_>) -> Result<(), EventContextError>;
    /// Run the validator, show its result, and report whether it passed.
    fn validate(&self) -> bool;
    /// Hand the current value to `on_saved`.
    fn save(&self, cx: &mut EventCx<'_>) -> Result<(), EventContextError>;
    /// Back to the initial value, errors and interaction cleared.
    fn reset(&self, cx: &mut EventCx<'_>) -> Result<(), EventContextError>;
    fn has_interacted_by_user(&self) -> bool;
    fn has_error(&self) -> bool;
}

/// The form's shared state; fields hold it weakly.
#[derive(Default)]
pub(crate) struct FormInner {
    active_attachment: Cell<Option<u64>>,
    next_attachment: Cell<u64>,
    attachment_error_pending: Cell<bool>,
    writer: RefCell<Option<WriterSource>>,
    /// Registered fields, in registration order (Flutter's insertion-ordered
    /// `Set<FormFieldState>`).
    fields: RefCell<Vec<Rc<dyn FormFieldEntry>>>,
    interacted: Cell<bool>,
    mode: Cell<AutovalidateMode>,
    on_changed: RefCell<Option<EventCallback>>,
    /// Set while `reset` visits its fields: a field's change notice then
    /// reports `on_changed` but defers autovalidation to the end, as
    /// Flutter's single rebuild after the loop does.
    resetting: Cell<bool>,
}

/// A typed diagnostic emitted when one [`FormHandle`] is requested by two
/// simultaneously mounted forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the FormHandle is already attached to another mounted Form")]
pub struct FormHandleAlreadyAttached;

/// Exclusive ownership of a mounted form handle. The generation makes stale
/// cleanup harmless after a later attachment has taken ownership.
struct FormAttachment {
    inner: Rc<FormInner>,
    generation: u64,
}

impl Drop for FormAttachment {
    fn drop(&mut self) {
        if self.inner.active_attachment.get() != Some(self.generation) {
            return;
        }
        self.inner.active_attachment.set(None);
        self.inner.writer.borrow_mut().take();
        self.inner.on_changed.borrow_mut().take();
        self.inner.fields.borrow_mut().clear();
        self.inner.interacted.set(false);
        self.inner.resetting.set(false);
    }
}

/// The form's imperative surface — Flutter's `FormState`, reached by a handle
/// the caller creates and passes to [`Form::handle`], or from [`Form::of`],
/// instead of a `GlobalKey<FormState>`.
///
/// Cheap to clone; every clone names the same form. Owner-thread only.
/// A handle binds to at most one mounted `Form` at a time. A simultaneous
/// duplicate is diagnosed and receives an isolated internal handle: it cannot
/// replace or later detach the first form's fields, callbacks or presentation
/// binding. Once the owner unmounts, the handle can be attached again.
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

    /// Take the duplicate-attachment diagnostic recorded during mounting.
    ///
    /// The refused form is isolated behind its own internal handle, so it
    /// cannot mutate this handle's owner. This drain is the typed counterpart
    /// to the tracing error, following the duplicate-`GlobalKey` diagnostic
    /// contract rather than panicking on caller-controlled input.
    pub fn take_attachment_error(&self) -> Option<FormHandleAlreadyAttached> {
        self.inner
            .attachment_error_pending
            .replace(false)
            .then_some(FormHandleAlreadyAttached)
    }

    fn try_attach(&self) -> Option<FormAttachment> {
        if self.inner.active_attachment.get().is_some() {
            return None;
        }
        let generation = self
            .inner
            .next_attachment
            .get()
            .checked_add(1)
            .expect("BUG: a FormHandle attachment generation cannot exhaust u64");
        self.inner.next_attachment.set(generation);
        self.inner.active_attachment.set(Some(generation));
        Some(FormAttachment {
            inner: Rc::clone(&self.inner),
            generation,
        })
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
    ///
    /// Takes the `&mut EventCx<'_>` of the event callback that saves (a
    /// submit button's press), and hands it to each `on_saved` (ADR-0086 §6):
    /// `.on_press(move |cx| form.save(cx))`.
    ///
    /// # Errors
    /// Refuses a detached form, a foreign presentation or a build-time call
    /// before dispatching any callback.
    /// The current field snapshot is checked before dispatch; if a callback
    /// detaches a later field, that field refuses its turn. Earlier callback
    /// effects are not rolled back.
    pub fn save(&self, cx: &mut EventCx<'_>) -> Result<(), EventContextError> {
        let fields = self.checked_fields(cx)?;
        for field in fields {
            field.save(cx)?;
        }
        Ok(())
    }

    /// Every field back to its initial value, its error and interaction
    /// cleared; then the form's interaction is cleared and `on_changed`
    /// runs — Flutter's `FormState.reset`. The caller's `&mut EventCx<'_>`
    /// reaches every `on_reset` and `on_changed`.
    ///
    /// # Errors
    /// Refuses a detached form, a foreign presentation or a build-time call
    /// before changing field state. User callbacks are not transactional.
    pub fn reset(&self, cx: &mut EventCx<'_>) -> Result<(), EventContextError> {
        let fields = self.checked_fields(cx)?;
        struct ResetGuard<'a> {
            resetting: &'a Cell<bool>,
            previous: bool,
        }
        impl Drop for ResetGuard<'_> {
            fn drop(&mut self) {
                self.resetting.set(self.previous);
            }
        }
        let guard = ResetGuard {
            resetting: &self.inner.resetting,
            previous: self.inner.resetting.replace(true),
        };
        for field in fields {
            field.reset(cx)?;
        }
        drop(guard);
        self.inner.interacted.set(false);
        self.field_did_change(cx);
        Ok(())
    }

    pub(crate) fn check_context(&self, cx: &EventCx<'_>) -> Result<(), EventContextError> {
        self.inner
            .writer
            .borrow()
            .as_ref()
            .ok_or(EventContextError::Detached)?
            .check_context(cx)
    }

    fn checked_fields(
        &self,
        cx: &EventCx<'_>,
    ) -> Result<Vec<Rc<dyn FormFieldEntry>>, EventContextError> {
        self.check_context(cx)?;
        let fields = self.fields();
        for field in &fields {
            field.check_context(cx)?;
        }
        Ok(fields)
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
    pub(crate) fn field_did_change(&self, cx: &mut EventCx<'_>) {
        let on_changed = self.inner.on_changed.borrow().clone();
        if let Some(on_changed) = on_changed {
            on_changed(cx);
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
    on_changed: Option<EventCallback>,
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

    /// Called when any field's value changes, and on `reset`, with the
    /// `&mut EventCx<'_>` of the event that changed it: the field's own edit
    /// callback, or the caller of [`FormFieldHandle::did_change`],
    /// [`FormFieldHandle::reset`] or [`FormHandle::reset`] (ADR-0086 §6).
    #[must_use]
    pub fn on_changed<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_changed = Some(event_callback(callback));
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
    /// The external handle actually owned by this state. `None` means the
    /// form owns an internal handle, including the isolated fallback used
    /// after an external duplicate is refused.
    bound_external: Option<FormHandle>,
    attachment: Option<FormAttachment>,
}

impl std::fmt::Debug for FormState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormState")
            .field("handle", &self.handle)
            .field("uses_external_handle", &self.bound_external.is_some())
            .finish_non_exhaustive()
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

    fn attach(view: &Form) -> Self {
        let requested = view.handle.clone().unwrap_or_default();
        if let Some(attachment) = requested.try_attach() {
            let state = Self {
                handle: requested,
                bound_external: view.handle.clone(),
                attachment: Some(attachment),
            };
            state.configure(view);
            return state;
        }

        tracing::error!(
            "FormHandle is already attached to a mounted Form; the duplicate Form uses an \
             isolated internal handle so it cannot mutate or detach the existing owner"
        );
        requested.inner.attachment_error_pending.set(true);
        let handle = FormHandle::new();
        let attachment = handle
            .try_attach()
            .expect("BUG: a fresh FormHandle has no existing attachment");
        let state = Self {
            handle,
            bound_external: None,
            attachment: Some(attachment),
        };
        state.configure(view);
        state
    }

    fn owns_requested_handle(&self, view: &Form) -> bool {
        match (&self.bound_external, &view.handle) {
            (Some(bound), Some(requested)) => bound.same_form(requested),
            (None, None) => true,
            _ => false,
        }
    }

    fn try_rebind(&mut self, view: &Form) {
        if self.owns_requested_handle(view) {
            return;
        }
        let requested = view.handle.clone().unwrap_or_default();
        let Some(attachment) = requested.try_attach() else {
            tracing::error!(
                "FormHandle is already attached to a mounted Form; retaining the current \
                 attachment until a later update can acquire the requested handle"
            );
            requested.inner.attachment_error_pending.set(true);
            return;
        };
        let writer = self.handle.inner.writer.borrow().clone();
        *requested.inner.writer.borrow_mut() = writer;
        let old_attachment = self.attachment.replace(attachment);
        self.handle = requested;
        self.bound_external.clone_from(&view.handle);
        drop(old_attachment);
    }
}

impl StatefulView for Form {
    type State = FormState;

    fn create_state(&self) -> FormState {
        FormState::attach(self)
    }
}

impl ViewState<Form> for FormState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.handle.inner.writer.borrow_mut() = Some(ctx.writer_source());
    }

    fn did_update_view(&mut self, _old_view: &Form, new_view: &Form) {
        // Acquire before releasing: a busy replacement cannot detach the
        // form from the handle that still owns its fields and writer.
        self.try_rebind(new_view);
        self.configure(new_view);
        // A reconfigured form rebuilds in Flutter, and its build runs the
        // form-level autovalidation.
        let fields = self.handle.fields();
        self.handle.autovalidate(&fields);
    }

    fn dispose(&mut self) {
        drop(self.attachment.take());
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
