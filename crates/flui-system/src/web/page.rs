//! The page a window's canvas is in, as the web providers reach it: through
//! the acquiring widget's [`WindowRef`], never through the browser's
//! globals, so a service acts on the document that shows its window.

use flui_platform::BrowserException;
use flui_platform::capability::WindowRef;
use js_sys::{Function, Reflect};
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
use wasm_bindgen::{JsCast as _, JsValue};
use web_sys::{Document, HtmlCanvasElement, Navigator, Node, Window};

/// Where the page's word on user activation comes from.
#[derive(Clone, Debug)]
pub(crate) enum Activation {
    /// `navigator.userActivation.isActive`.
    Browser,
    /// A test's answer, standing in for a gesture no test can make; the
    /// test may change it while a call waits on the browser.
    #[cfg(test)]
    Stated(std::rc::Rc<std::cell::Cell<bool>>),
}

/// A window's page: its canvas, the canvas's document and that document's
/// window.
#[derive(Clone, Debug)]
pub(crate) struct Page {
    canvas: HtmlCanvasElement,
    document: Document,
    view: Window,
    activation: Activation,
}

impl Page {
    /// The page `window`'s canvas is in; `None` for a window whose handle
    /// names no canvas (a headless window) or that has retired, and for a
    /// canvas in no document with a window.
    pub(crate) fn of(window: &WindowRef) -> Option<Self> {
        let handle = window.window_handle().ok()?;
        let RawWindowHandle::WebCanvas(canvas) = handle.as_raw() else {
            return None;
        };
        // SAFETY: a `WebCanvasWindowHandle` points at the `JsValue` of an
        // `HtmlCanvasElement` (raw-window-handle's contract for it), which
        // the web host's surface target owns and keeps for the borrow of
        // `window` that `handle` carries; the value is cloned (a new
        // reference to the same canvas) before that borrow ends.
        let canvas = unsafe { canvas.obj.cast::<JsValue>().as_ref() }.clone();
        Self::for_canvas(canvas.dyn_into().ok()?, Activation::Browser)
    }

    /// The page `canvas` is in, reading user activation from `activation`.
    pub(crate) fn for_canvas(canvas: HtmlCanvasElement, activation: Activation) -> Option<Self> {
        let document = canvas.owner_document()?;
        let view = document.default_view()?;
        Some(Self {
            canvas,
            document,
            view,
            activation,
        })
    }

    /// Whether the page is handling a user's gesture now (transient
    /// activation), which a picker, a share and a new window require.
    ///
    /// `true` where the browser has no `navigator.userActivation` (older
    /// engines): the call is made, and the browser refuses it itself if it
    /// must.
    pub(crate) fn is_activated(&self) -> bool {
        self.reported_activation().unwrap_or(true)
    }

    /// What the browser says of the page's transient activation:
    /// `navigator.userActivation.isActive`, or `None` where the browser has
    /// no `navigator.userActivation`.
    pub(crate) fn reported_activation(&self) -> Option<bool> {
        match &self.activation {
            Activation::Browser => property(&self.navigator(), "userActivation")
                .and_then(|activation| property(&activation, "isActive"))
                .and_then(|active| active.as_bool()),
            #[cfg(test)]
            Activation::Stated(active) => Some(active.get()),
        }
    }

    pub(crate) fn document(&self) -> &Document {
        &self.document
    }

    pub(crate) fn view(&self) -> &Window {
        &self.view
    }

    pub(crate) fn navigator(&self) -> Navigator {
        self.view.navigator()
    }

    /// Where the providers put the elements they show (a dialog, a file
    /// input): beside the canvas, in the window's own part of the page, or
    /// in the document's body when the canvas has no parent.
    pub(crate) fn host(&self) -> Option<Node> {
        self.canvas
            .parent_node()
            .or_else(|| self.document.body().map(Node::from))
    }

    /// Whether the browser's file input fires `cancel` when its chooser is
    /// dismissed with nothing picked (Chromium 113, Firefox 91, Safari
    /// 16.4), so that an input ends without a bound on the window's focus.
    ///
    /// No API says whether the event is fired, so a method that arrived no
    /// earlier in every engine stands for it: `Headers.prototype.getSetCookie`
    /// (Chromium 113, Firefox 112, Safari 17). A browser that has it fires
    /// `cancel`; one without it is taken for one that may not, which costs a
    /// Firefox before 112 or a Safari before 17 only the bound it does not
    /// need.
    pub(crate) fn input_fires_cancel(&self) -> bool {
        property(self.view.as_ref(), "Headers")
            .and_then(|headers| property(&headers, "prototype"))
            .and_then(|prototype| method(&prototype, "getSetCookie"))
            .is_some()
    }

    /// The window's method `name`, when the browser has it: the File System
    /// Access pickers exist only in Chromium, in a secure context.
    pub(crate) fn window_method(&self, name: &str) -> Option<Function> {
        method(self.view.as_ref(), name)
    }
}

/// `target`'s property `name`, when it is neither `undefined` nor `null`;
/// read through `Reflect`, so a browser without it does not throw.
pub(crate) fn property(target: &JsValue, name: &str) -> Option<JsValue> {
    Reflect::get(target, &JsValue::from_str(name))
        .ok()
        .filter(|value| !value.is_undefined() && !value.is_null())
}

/// `target`'s method `name`, when it has one.
pub(crate) fn method(target: &JsValue, name: &str) -> Option<Function> {
    property(target, name)?.dyn_into::<Function>().ok()
}

/// Call `target`'s method `function` with `argument`, as a promise; the
/// exception it throws at once otherwise.
pub(crate) fn call_promise(
    function: &Function,
    target: &JsValue,
    argument: &JsValue,
) -> Result<js_sys::Promise, BrowserException> {
    function
        .call1(target, argument)
        .map_err(|thrown| exception(&thrown))?
        .dyn_into::<js_sys::Promise>()
        .map_err(|_| BrowserException::Type)
}

/// The exception a rejection or a throw carries, by its `name`.
pub(crate) fn exception(value: &JsValue) -> BrowserException {
    property(value, "name")
        .and_then(|name| name.as_string())
        .map_or(BrowserException::Other, |name| {
            BrowserException::from_name(&name)
        })
}

/// Set `target`'s property `name` to `value`. Setting a property of a plain
/// object the provider made cannot fail.
pub(crate) fn set(target: &JsValue, name: &str, value: &JsValue) {
    let _ = Reflect::set(target, &JsValue::from_str(name), value);
}
