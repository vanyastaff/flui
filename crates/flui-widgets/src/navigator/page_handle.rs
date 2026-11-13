//! [`PageHandle`]: the route a widget is on, reached from its build context
//! (ADR-0330 §3).
//!
//! Flutter's counterpart is `ModalRoute.of(context)` with the
//! `Navigator.pop(context)` family and `showDialog(context:)` on it. Every
//! modal route (a `PageRoute` or a `PopupRoute`) places a [`PageScope`] above
//! its page when it builds; [`PageHandle::of`] reads the nearest one.
//!
//! The handle is bound to its route, not to "the top of the stack": a
//! callback that runs after another route covered this one does not pop the
//! cover. That is the identity a Flutter context gives for free, and the
//! reason the handle carries the route's id.

use flui_view::element::ElementKind;
use flui_view::prelude::*;

use super::lifecycle::Terminal;
use super::navigator::NavigatorHandle;
use super::page_route::PopupRoute;
use super::result::RouteResult;
use super::route::{Route, RouteId};

/// The route a widget is on: a page of its `Router`, or a popup over one.
///
/// Acquired with [`PageHandle::of`] in `build` or a lifecycle hook, and moved
/// into the callbacks that navigate. Owner-affine (`!Send`), like the
/// navigator it names. Every method is a no-op answer once the route has
/// left its navigator.
///
/// # Example
///
/// ```
/// use flui_widgets::prelude::*;
/// use flui_widgets::{PageHandle, RawButton, Text};
///
/// #[derive(Clone, StatelessView)]
/// struct Close;
///
/// impl StatelessView for Close {
///     fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
///         let page = PageHandle::of(ctx);
///         RawButton::new(Text::new("Close")).on_pressed(move |_ctx| {
///             if let Some(page) = &page {
///                 let _ = page.pop();
///             }
///         })
///     }
/// }
/// # let _ = Close;
/// ```
#[derive(Clone)]
pub struct PageHandle {
    navigator: NavigatorHandle,
    route: RouteId,
}

impl PageHandle {
    /// The route `ctx` is on, or `None` outside every route.
    ///
    /// Registers a dependency on the route's scope, so a widget that keeps
    /// the handle in its state reads it again in `did_change_dependencies`.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> Option<Self> {
        ctx.depend_on::<PageScope, _>(|scope| scope.page.clone())
    }

    /// Whether this route is on top of its navigator.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.navigator.is_mounted() && self.navigator.current() == Some(self.route)
    }

    /// Whether a route sits below this one for a pop to return to: Flutter's
    /// `ModalRoute.impliesAppBarDismissal`, what decides an app bar's back
    /// button. `false` for a Router's last page, and once the route is gone.
    #[must_use]
    pub fn can_pop(&self) -> bool {
        self.navigator.is_mounted() && self.navigator.route_can_pop(self.route)
    }

    /// Ask this route to pop, honouring its `PopScope`s and local history:
    /// what a back button and the system back key do.
    ///
    /// Returns whether the request was handled. `false` when this route is
    /// not on top (a popup covers it, or it is gone), when it is a Router's
    /// last page, or while the UI runtime builds (ADR-0330 §3).
    pub fn maybe_pop(&self) -> bool {
        if self.editable().is_err() || !self.is_current() {
            return false;
        }
        self.navigator.maybe_pop()
    }

    /// Pop this route with no value: its [`RouteResult`] resolves to
    /// `Ok(None)`.
    ///
    /// # Errors
    ///
    /// [`PageError::DuringBuild`] while the UI runtime builds, and
    /// [`PageError::Gone`] once the route has left its navigator.
    ///
    /// `Ok(false)` when the route is present but not on top, or is a Router's
    /// last page; nothing is popped.
    pub fn pop(&self) -> Result<bool, PageError> {
        self.editable()?;
        Ok(self.is_current() && self.navigator.pop())
    }

    /// Pop this route, delivering `value` to whoever awaits its
    /// [`RouteResult`].
    ///
    /// A value of another type than the route's result completes it with
    /// [`RouteResultError::TypeMismatch`](crate::RouteResultError::TypeMismatch).
    ///
    /// # Errors
    ///
    /// As [`pop`](Self::pop). A refused or unpopped `value` is dropped after
    /// every navigator borrow is released.
    pub fn pop_with<T: 'static>(&self, value: T) -> Result<bool, PageError> {
        self.editable()?;
        if !self.is_current() {
            drop(value);
            return Ok(false);
        }
        Ok(self.navigator.pop_with(value))
    }

    /// Push `popup` over this route's navigator: a dialog, a menu or a sheet,
    /// as Flutter's `showDialog(context:)` pushes a `DialogRoute`. The popup
    /// has no path, so it never enters a Router's stack or URL.
    ///
    /// # Errors
    ///
    /// [`PageError::DuringBuild`] while the UI runtime builds, and
    /// [`PageError::Gone`] once this route has left its navigator. The popup
    /// is disposed unpushed.
    pub fn push_popup<T: 'static>(
        &self,
        popup: PopupRoute<T>,
    ) -> Result<RouteResult<T>, PageError> {
        let refusal = self.editable().err().or_else(|| {
            (!self.navigator.is_mounted() || !self.navigator.route_is_active(self.route))
                .then_some(PageError::Gone)
        });
        if let Some(refusal) = refusal {
            let mut popup = popup;
            Route::dispose(&mut popup);
            drop(popup);
            return Err(refusal);
        }
        Ok(self.navigator.push(popup))
    }

    /// Refuse a mutation while building, then one of a route that has left.
    fn editable(&self) -> Result<(), PageError> {
        flui_foundation::MutatedDuringBuild::check(self.navigator.is_building(), "PageHandle")
            .map_err(|_| PageError::DuringBuild)?;
        if self.navigator.is_mounted() && self.navigator.route_is_active(self.route) {
            Ok(())
        } else {
            Err(PageError::Gone)
        }
    }
}

impl PartialEq for PageHandle {
    fn eq(&self, other: &Self) -> bool {
        self.route == other.route && self.navigator.same_navigator(&other.navigator)
    }
}

impl Eq for PageHandle {}

impl std::fmt::Debug for PageHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageHandle")
            .field("route", &self.route)
            .finish_non_exhaustive()
    }
}

/// Why a [`PageHandle`] did not act.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PageError {
    /// The route has been popped, replaced or removed, or its navigator
    /// unmounted.
    #[error("the route this handle names has left its navigator")]
    Gone,
    /// The call was made while the UI runtime was building, and refused:
    /// `build` describes the tree and drives nothing (ADR-0330 §3). Navigate
    /// from an event callback, a lifecycle hook or a post-frame callback.
    #[error("a page was navigated during build")]
    DuringBuild,
}

/// Provides a modal route's [`PageHandle`] to [`PageHandle::of`] lookups in
/// its subtree, Flutter's `_ModalScopeStatus`.
///
/// It also holds what the handle answered when the route's entry built, so a
/// dependent rebuilds when the route becomes or stops being current, or gains
/// or loses the route below it: the navigator rebuilds a route's entry each
/// time it tells the route of a new neighbour.
pub(crate) struct PageScope {
    page: PageHandle,
    status: PageStatus,
    child: Terminal<BoxedView>,
}

/// What a [`PageHandle`] answered when its route's entry built.
#[derive(Clone, Copy, PartialEq, Eq)]
struct PageStatus {
    is_current: bool,
    can_pop: bool,
}

impl PageScope {
    pub(crate) fn new(navigator: NavigatorHandle, route: RouteId, child: BoxedView) -> Self {
        let page = PageHandle { navigator, route };
        let status = PageStatus {
            is_current: page.is_current(),
            can_pop: page.can_pop(),
        };
        Self {
            page,
            status,
            child: Terminal::new(child),
        }
    }
}

impl Clone for PageScope {
    fn clone(&self) -> Self {
        Self {
            page: self.page.clone(),
            status: self.status,
            child: Terminal::new(self.child.clone()),
        }
    }
}

impl Drop for PageScope {
    fn drop(&mut self) {
        let child = self.child.withdraw();
        drop(child);
    }
}

impl InheritedView for PageScope {
    type Data = PageHandle;

    fn data(&self) -> &Self::Data {
        &self.page
    }

    fn child(&self) -> &dyn View {
        &*self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.page != old.page || self.status != old.status
    }
}

impl View for PageScope {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::inherited(self)
    }
}
