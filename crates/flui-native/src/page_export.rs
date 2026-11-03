//! Connections a page script opens to the application through a global (ADR-0288 §6): the
//! web's endpoint for a development agent, and the timer that drives what it serves.
//!
//! [`PageExport::try_new`] defines `globalThis[name]` as
//! `{ version, connect(onLine) }`. `connect` hands the application a [`PageLine`] over
//! `onLine` and returns `{ version, send(line), close() }`; a connection the application
//! refuses makes `connect` throw the reason. Each line the application writes reaches `onLine`
//! as a string, in order, and `onLine(null)` tells the script the application ended the
//! connection. A WebDriver BiDi driver passes a channel as `onLine`, so each line reaches it as
//! a `script.message` event, and calls `send` with `script.callFunction`.
//!
//! Every call from the page arrives on the page's event loop with nothing of this module
//! borrowed, and every call into the page is made the same way: the application may write
//! from inside [`PageConnection::receive`], and the script may send from inside its `onLine`.
//! Dropping the export deletes the global and ends every connection: the application hears
//! [`PageConnection::closed`], the script `onLine(null)`, and the functions a script kept throw
//! that the export is gone.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::rc::{Rc, Weak};
use std::time::Duration;

use js_sys::{Function, JsString, Object, Reflect};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::PlatformError;

/// The shape of the export and of its connections, which both carry it as `version`: a script
/// reads it before it relies on more than this version gives.
pub const PAGE_EXPORT_VERSION: u32 = 1;

/// What the application does with the connections a [`PageExport`] receives.
pub trait PageExportHandler {
    /// A script connected; `line` reaches its `onLine`. `Err(reason)` refuses the
    /// connection: the script's `connect` throws `reason`.
    ///
    /// # Errors
    ///
    /// The reason the application refuses the connection.
    fn connect(&self, line: PageLine) -> Result<Rc<dyn PageConnection>, String>;
}

/// One connection, as the application hears what its script does.
pub trait PageConnection {
    /// The script sent `line`. A line longer than the export's `max_line` arrives cut after
    /// `max_line + 1` UTF-16 units: still longer than the bound, and copied no further.
    fn receive(&self, line: String);

    /// The script closed the connection, or the export went. Called once, and only for a
    /// connection the application had not ended; nothing is received after it.
    fn closed(&self);
}

/// Why a line did not reach its script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageLineClosed;

impl fmt::Display for PageLineClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the page connection is closed")
    }
}

impl std::error::Error for PageLineClosed {}

/// The `onLine` a connection's script gave, and whether lines still reach it.
struct LineState {
    on_line: Function,
    open: Cell<bool>,
}

impl LineState {
    /// Tell the script the application ended the connection, once.
    fn end(&self) {
        if self.open.replace(false) {
            // The connection is over whatever `onLine` does with the news.
            let _ = self.on_line.call1(&JsValue::NULL, &JsValue::NULL);
        }
    }
}

/// The application's end of one connection: the `onLine` its script connected with.
pub struct PageLine(Rc<LineState>);

impl PageLine {
    /// Hand `line` to the script's `onLine`. An `onLine` that throws ends the connection: the
    /// script's `send` throws after it.
    ///
    /// # Errors
    ///
    /// [`PageLineClosed`] when the connection had ended, or `onLine` threw.
    pub fn send(&self, line: &str) -> Result<(), PageLineClosed> {
        if !self.0.open.get() {
            return Err(PageLineClosed);
        }
        if self
            .0
            .on_line
            .call1(&JsValue::NULL, &JsValue::from_str(line))
            .is_err()
        {
            self.0.open.set(false);
            return Err(PageLineClosed);
        }
        Ok(())
    }

    /// End the connection: the script's `onLine(null)`, once. Its `send` throws after it, and
    /// the application hears no [`PageConnection::closed`] for it.
    pub fn close(&self) {
        self.0.end();
    }
}

impl fmt::Debug for PageLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageLine")
            .field("open", &self.0.open.get())
            .finish_non_exhaustive()
    }
}

/// A connection the export holds: where its script's calls go, and its line.
struct Held {
    connection: Rc<dyn PageConnection>,
    line: Rc<LineState>,
}

/// What the export's functions share.
struct Shared {
    handler: Box<dyn PageExportHandler>,
    max_line: u32,
    connections: RefCell<BTreeMap<u32, Held>>,
    /// The next connection's ID; `None` once every ID was given out.
    next: Cell<Option<u32>>,
}

/// A function the page calls; a missing argument arrives `undefined`.
type PageFunction = Closure<dyn Fn(JsValue, JsValue) -> Result<JsValue, JsValue>>;

impl Shared {
    fn connect(
        &self,
        on_line: JsValue,
        send: &Function,
        close: &Function,
    ) -> Result<JsValue, JsValue> {
        let on_line = on_line
            .dyn_into::<Function>()
            .map_err(|_| error("connect takes a function: onLine"))?;
        let id = self
            .next
            .get()
            .ok_or_else(|| error("the export has made every connection it can"))?;
        self.next.set(id.checked_add(1));
        // Forgotten here once the application ended them or their script stopped listening.
        self.connections
            .borrow_mut()
            .retain(|_, held| held.line.open.get());
        let line = Rc::new(LineState {
            on_line,
            open: Cell::new(true),
        });
        // Called with nothing borrowed: the handler is the application's code.
        let connection = self
            .handler
            .connect(PageLine(Rc::clone(&line)))
            .map_err(|reason| error(&reason))?;
        self.connections
            .borrow_mut()
            .insert(id, Held { connection, line });
        let object = Object::new();
        let id = JsValue::from(id);
        set(&object, "version", &JsValue::from(PAGE_EXPORT_VERSION))?;
        set(&object, "send", &send.bind1(&JsValue::NULL, &id))?;
        set(&object, "close", &close.bind1(&JsValue::NULL, &id))?;
        Ok(object.into())
    }

    /// The connection `id` names, while it is open.
    fn open(&self, id: &JsValue) -> Option<Rc<dyn PageConnection>> {
        let id = connection_id(id)?;
        let connections = self.connections.borrow();
        let held = connections.get(&id)?;
        held.line.open.get().then(|| Rc::clone(&held.connection))
    }

    fn send(&self, id: &JsValue, line: &JsValue) -> Result<(), JsValue> {
        let connection = self
            .open(id)
            .ok_or_else(|| error("the connection is closed"))?;
        let text = line
            .dyn_ref::<JsString>()
            .ok_or_else(|| error("send takes a string: one line"))?;
        // Copied no further than the bound shows: a longer line is refused all the same.
        let text = if text.length() > self.max_line {
            text.slice(0, self.max_line.saturating_add(1))
        } else {
            text.clone()
        };
        // Called with nothing borrowed: the application may write, and its script send again.
        connection.receive(String::from(text));
        Ok(())
    }

    fn close(&self, id: &JsValue) {
        let held = connection_id(id).and_then(|id| self.connections.borrow_mut().remove(&id));
        if let Some(held) = held
            && held.line.open.replace(false)
        {
            held.connection.closed();
        }
    }

    /// End every connection: the application hears each close, and each script `onLine(null)`.
    fn end_all(&self) {
        let connections = std::mem::take(&mut *self.connections.borrow_mut());
        for held in connections.into_values() {
            if held.line.open.get() {
                held.line.end();
                held.connection.closed();
            }
        }
    }
}

/// The page export: `globalThis[name]` while it lives. See the [module docs](self).
pub struct PageExport {
    name: String,
    shared: Rc<Shared>,
    /// The functions the page calls; kept after the export goes, so that a script that kept
    /// one is told the export is gone rather than that a Rust closure was dropped.
    functions: Option<[PageFunction; 3]>,
}

impl PageExport {
    /// Define `globalThis[name]` and hand each connection a script opens to `handler`. A line
    /// a script sends reaches the application whole up to `max_line` UTF-16 units.
    ///
    /// # Errors
    ///
    /// [`PlatformError::Init`] when `name` is empty or already defined on the page, or the
    /// page refused to define it.
    pub fn try_new(
        name: &str,
        max_line: usize,
        handler: impl PageExportHandler + 'static,
    ) -> Result<Self, PlatformError> {
        let failed = |message: String| PlatformError::Init { message };
        if name.is_empty() {
            return Err(failed("a page export needs a name".to_owned()));
        }
        let global = js_sys::global();
        let key = JsValue::from_str(name);
        if Reflect::has(&global, &key).unwrap_or(true) {
            return Err(failed(format!("the page already defines `{name}`")));
        }
        let shared = Rc::new(Shared {
            handler: Box::new(handler),
            max_line: u32::try_from(max_line).unwrap_or(u32::MAX),
            connections: RefCell::new(BTreeMap::new()),
            next: Cell::new(Some(1)),
        });

        let sending = Rc::downgrade(&shared);
        let send = PageFunction::new(move |id: JsValue, line: JsValue| {
            live(&sending)?.send(&id, &line)?;
            Ok(JsValue::UNDEFINED)
        });
        let closing = Rc::downgrade(&shared);
        let close = PageFunction::new(move |id: JsValue, _: JsValue| {
            if let Some(shared) = closing.upgrade() {
                shared.close(&id);
            }
            Ok(JsValue::UNDEFINED)
        });
        let connecting = Rc::downgrade(&shared);
        let send_function: Function = send.as_ref().unchecked_ref::<Function>().clone();
        let close_function: Function = close.as_ref().unchecked_ref::<Function>().clone();
        let connect = PageFunction::new(move |on_line: JsValue, _: JsValue| {
            live(&connecting)?.connect(on_line, &send_function, &close_function)
        });

        let export = Object::new();
        let defined = set(&export, "version", &JsValue::from(PAGE_EXPORT_VERSION))
            .and_then(|()| set(&export, "connect", connect.as_ref()))
            .and_then(|()| define(&global, &key, &export));
        if let Err(error) = defined {
            return Err(failed(format!(
                "the page refused the export `{name}`: {}",
                describe(&error)
            )));
        }
        Ok(Self {
            name: name.to_owned(),
            shared,
            functions: Some([connect, send, close]),
        })
    }
}

impl Drop for PageExport {
    fn drop(&mut self) {
        let _ = Reflect::delete_property(&js_sys::global(), &JsValue::from_str(&self.name));
        self.shared.end_all();
        if let Some(functions) = self.functions.take() {
            // Each holds only a `Weak` of the export, gone now.
            for function in functions {
                function.forget();
            }
        }
    }
}

impl fmt::Debug for PageExport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageExport")
            .field("name", &self.name)
            .field("connections", &self.shared.connections.borrow().len())
            .finish_non_exhaustive()
    }
}

/// A timer on the page's event loop: its body runs once each time a delay it was armed with
/// passes, one pending run at a time. Dropping it clears the pending run.
pub struct PageTimer {
    closure: Closure<dyn Fn()>,
    /// The pending run's handle (a number in a browser, an object in Node).
    pending: Rc<RefCell<Option<JsValue>>>,
    set_timeout: Function,
    clear_timeout: Function,
}

impl PageTimer {
    /// A timer that runs `body`, unarmed.
    ///
    /// # Errors
    ///
    /// [`PlatformError::Init`] when the page has no `setTimeout` or `clearTimeout`.
    pub fn try_new(body: impl Fn() + 'static) -> Result<Self, PlatformError> {
        let global = js_sys::global();
        let function = |name: &str| {
            Reflect::get(&global, &JsValue::from_str(name))
                .ok()
                .and_then(|function| function.dyn_into::<Function>().ok())
                .ok_or_else(|| PlatformError::Init {
                    message: format!("the page has no {name}"),
                })
        };
        let set_timeout = function("setTimeout")?;
        let clear_timeout = function("clearTimeout")?;
        let pending: Rc<RefCell<Option<JsValue>>> = Rc::default();
        let fired = Rc::clone(&pending);
        let closure = Closure::<dyn Fn()>::new(move || {
            // Let go before the body runs: it may arm the timer again.
            let handle = fired.borrow_mut().take();
            drop(handle);
            body();
        });
        Ok(Self {
            closure,
            pending,
            set_timeout,
            clear_timeout,
        })
    }

    /// Run the body once `delay` has passed, in place of a pending run. The delay is rounded
    /// up to a whole millisecond, so the body never runs early, and held at the longest the
    /// page waits (`2³¹ − 1` ms, about 24.8 days).
    ///
    /// # Errors
    ///
    /// [`PlatformError::Init`] when the page refused to set the timeout; nothing is pending
    /// then.
    pub fn arm(&self, delay: Duration) -> Result<(), PlatformError> {
        self.cancel();
        let millis = u32::try_from(delay.as_nanos().div_ceil(1_000_000))
            .unwrap_or(u32::MAX)
            .min(i32::MAX.unsigned_abs());
        let handle = self
            .set_timeout
            .call2(
                &JsValue::NULL,
                self.closure.as_ref(),
                &JsValue::from(millis),
            )
            .map_err(|error| PlatformError::Init {
                message: format!("the page refused a timeout: {}", describe(&error)),
            })?;
        *self.pending.borrow_mut() = Some(handle);
        Ok(())
    }

    /// Clear the pending run, if any.
    pub fn cancel(&self) {
        let handle = self.pending.borrow_mut().take();
        if let Some(handle) = handle {
            let _ = self.clear_timeout.call1(&JsValue::NULL, &handle);
        }
    }
}

impl Drop for PageTimer {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl fmt::Debug for PageTimer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageTimer")
            .field("armed", &self.pending.borrow().is_some())
            .finish_non_exhaustive()
    }
}

/// The export's state while it lives.
fn live(shared: &Weak<Shared>) -> Result<Rc<Shared>, JsValue> {
    shared
        .upgrade()
        .ok_or_else(|| error("the page export is gone"))
}

/// The ID a connection's bound functions carry.
fn connection_id(id: &JsValue) -> Option<u32> {
    let id = id.as_f64()?;
    // Exact: an ID the export gave is a whole number it can hold.
    if id.fract() != 0.0 || !(1.0..=f64::from(u32::MAX)).contains(&id) {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a whole number within u32's range, checked above"
    )]
    let id = id as u32;
    Some(id)
}

/// An `Error` a page call throws.
fn error(reason: &str) -> JsValue {
    js_sys::Error::new(reason).into()
}

/// What a page error says, for a message of this crate's.
fn describe(error: &JsValue) -> String {
    error
        .dyn_ref::<js_sys::Error>()
        .map(|error| String::from(error.message()))
        .or_else(|| error.as_string())
        .unwrap_or_else(|| "an error with no message".to_owned())
}

fn set(object: &Object, key: &str, value: &JsValue) -> Result<(), JsValue> {
    if Reflect::set(object, &JsValue::from_str(key), value)? {
        Ok(())
    } else {
        Err(error(&format!("`{key}` could not be set")))
    }
}

/// Define `key` on `global` as `value`: configurable, so dropping the export can delete it.
fn define(global: &Object, key: &JsValue, value: &JsValue) -> Result<(), JsValue> {
    let descriptor = Object::new();
    set(&descriptor, "value", value)?;
    set(&descriptor, "configurable", &JsValue::TRUE)?;
    if Reflect::define_property(global, key, &descriptor)? {
        Ok(())
    } else {
        Err(error("the page refused to define it"))
    }
}
