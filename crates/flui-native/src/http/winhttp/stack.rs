//! WinHTTP itself, in asynchronous mode under the `blocking` module's model:
//! the request's thread starts each operation and waits on its [`Gate`] for
//! the status callback to complete it.

// This module calls the operating system directly, which has no safe wrapper:
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use std::sync::{Arc, OnceLock};

use flui_platform::http::{HttpError, HttpRequest, StatusCode};
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, GetLastError};
use windows::Win32::Networking::WinHttp::{
    URL_COMPONENTS, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ASYNC_RESULT,
    WINHTTP_CALLBACK_FLAG_HEADERS_AVAILABLE, WINHTTP_CALLBACK_FLAG_READ_COMPLETE,
    WINHTTP_CALLBACK_FLAG_REQUEST_ERROR, WINHTTP_CALLBACK_FLAG_SECURE_FAILURE,
    WINHTTP_CALLBACK_FLAG_SENDREQUEST_COMPLETE, WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING,
    WINHTTP_CALLBACK_STATUS_HEADERS_AVAILABLE, WINHTTP_CALLBACK_STATUS_READ_COMPLETE,
    WINHTTP_CALLBACK_STATUS_REQUEST_ERROR, WINHTTP_CALLBACK_STATUS_SECURE_FAILURE,
    WINHTTP_CALLBACK_STATUS_SENDREQUEST_COMPLETE, WINHTTP_FLAG_ASYNC, WINHTTP_FLAG_SECURE,
    WINHTTP_INTERNET_SCHEME_HTTPS, WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_CONTEXT_VALUE,
    WINHTTP_OPTION_ENABLE_HTTP_PROTOCOL, WINHTTP_PROTOCOL_FLAG_HTTP2, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_RAW_HEADERS_CRLF, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpCrackUrl, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetStatusCallback,
    WinHttpSetTimeouts,
};
use windows::core::{PCWSTR, PWSTR};

use super::gate::{Gate, Outcome};
use super::wire::{self, Stage};
use crate::http::blocking::{Abort, Connection, Head, Stack};
use crate::owner::panic_boundary::{AfterPanic, os_entry};

/// How long a connection may take to open, a send to go out, and a read to
/// answer.
const TIMEOUT_MS: i32 = 30_000;

/// The size of each body read.
const READ_BUFFER: usize = 16 * 1024;

/// What the status callback hears of a request: its operations' ends, a
/// failed secure check, and its handle's last notification.
const NOTIFICATIONS: u32 = WINHTTP_CALLBACK_FLAG_SENDREQUEST_COMPLETE
    | WINHTTP_CALLBACK_FLAG_HEADERS_AVAILABLE
    | WINHTTP_CALLBACK_FLAG_READ_COMPLETE
    | WINHTTP_CALLBACK_FLAG_REQUEST_ERROR
    | WINHTTP_CALLBACK_FLAG_SECURE_FAILURE
    // `WINHTTP_CALLBACK_FLAG_HANDLES` names its own status as its flag.
    | WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING;

/// `WinHttpSetStatusCallback`'s answer when it failed.
const INVALID_STATUS_CALLBACK: usize = usize::MAX;

/// One WinHTTP session, opened on the first request: the system's proxy
/// settings and trust store, one connection pool.
pub(super) struct Sessions {
    session: OnceLock<Result<Handle, String>>,
}

impl Sessions {
    pub(super) fn new() -> Self {
        Self {
            session: OnceLock::new(),
        }
    }
}

/// An open WinHTTP handle, as an address, closed on drop.
struct Handle(usize);

impl Drop for Handle {
    fn drop(&mut self) {
        close(self.0);
    }
}

fn close(handle: usize) {
    // SAFETY: `handle` is an open WinHTTP handle that only this call closes:
    // a `Handle` closes on drop alone, and a request's `Gate` closes its
    // handle once. A close cannot fail in a way left to handle.
    let _ = unsafe { WinHttpCloseHandle(handle as *mut core::ffi::c_void) };
}

/// The calling thread's last error, WinHTTP's code for its failed call.
fn last_error() -> u32 {
    // SAFETY: reads this thread's last-error value; no preconditions.
    unsafe { GetLastError() }.0
}

/// The WinHTTP code behind a failed call's error (an `HRESULT` of the Win32
/// facility).
fn code(error: &windows::core::Error) -> u32 {
    // The low word of a Win32-facility `HRESULT` is the Win32 code; the cast
    // keeps the bits.
    u32::from_ne_bytes(error.code().0.to_ne_bytes()) & 0xFFFF
}

/// `text` as a NUL-ended wide string.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn open_session() -> Result<Handle, String> {
    let agent = wide("FLUI");
    // SAFETY: the agent is a live NUL-ended wide string; no proxy names are
    // given with the automatic proxy.
    let raw = unsafe {
        WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            WINHTTP_FLAG_ASYNC,
        )
    };
    if raw.is_null() {
        return Err(format!("opening a session: WinHTTP error {}", last_error()));
    }
    let session = Handle(raw as usize);
    // SAFETY: `raw` is the open session.
    unsafe { WinHttpSetTimeouts(raw, 0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS) }
        .map_err(|error| format!("setting the timeouts: WinHTTP error {}", code(&error)))?;
    let http2 = WINHTTP_PROTOCOL_FLAG_HTTP2.to_ne_bytes();
    // SAFETY: `raw` is the open session and the option takes a `u32`. A
    // system without HTTP/2 refuses it and speaks HTTP/1.1.
    let _ = unsafe {
        WinHttpSetOption(
            Some(raw.cast_const()),
            WINHTTP_OPTION_ENABLE_HTTP_PROTOCOL,
            Some(&http2),
        )
    };
    Ok(session)
}

/// The part of `url` (a wide string) WinHTTP's crack pointed at.
///
/// # Safety
///
/// `at` and `length` are what `WinHttpCrackUrl` set for `url`: null, or a
/// pointer into `url` with that many code units after it.
unsafe fn piece(_url: &[u16], at: PWSTR, length: u32) -> &[u16] {
    if at.is_null() || length == 0 {
        return &[];
    }
    // SAFETY: the caller's contract.
    unsafe { std::slice::from_raw_parts(at.0, length as usize) }
}

impl Stack for Sessions {
    fn open(&self, request: &HttpRequest) -> Result<Box<dyn Connection>, HttpError> {
        let url = request.url();
        let invalid = || HttpError::InvalidUrl {
            url: url.to_owned(),
        };
        let scheme = url.split_once("://").map(|(scheme, _)| scheme);
        if !scheme.is_some_and(|scheme| {
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
        }) {
            return Err(invalid());
        }
        let session = self
            .session
            .get_or_init(open_session)
            .as_ref()
            .map_err(|detail| HttpError::Host {
                detail: detail.clone(),
            })?;
        let wide_url: Vec<u16> = url.encode_utf16().collect();
        let mut parts = URL_COMPONENTS {
            dwStructSize: u32::try_from(size_of::<URL_COMPONENTS>())
                .expect("BUG: URL_COMPONENTS is a few dozen bytes"),
            dwHostNameLength: u32::MAX,
            dwUrlPathLength: u32::MAX,
            dwExtraInfoLength: u32::MAX,
            ..URL_COMPONENTS::default()
        };
        // SAFETY: `parts` asks for pointers into `wide_url`, which outlives
        // every use of them below.
        unsafe { WinHttpCrackUrl(&wide_url, 0, &raw mut parts) }.map_err(|_| invalid())?;
        // SAFETY: each pointer and length is what the crack set for
        // `wide_url`.
        let (host, path, extra) = unsafe {
            (
                piece(&wide_url, parts.lpszHostName, parts.dwHostNameLength),
                piece(&wide_url, parts.lpszUrlPath, parts.dwUrlPathLength),
                piece(&wide_url, parts.lpszExtraInfo, parts.dwExtraInfoLength),
            )
        };
        if host.is_empty() {
            return Err(invalid());
        }
        let host: Vec<u16> = host.iter().copied().chain([0]).collect();
        let object = wire::object_name(path, extra);
        let verb = wide(request.method().as_str());

        // SAFETY: the session is open and the host a NUL-ended wide string.
        let connect = unsafe {
            WinHttpConnect(
                session.0 as *mut core::ffi::c_void,
                PCWSTR(host.as_ptr()),
                parts.nPort,
                0,
            )
        };
        if connect.is_null() {
            return Err(wire::failure(last_error(), Stage::Open, 0, url));
        }
        let connect = Handle(connect as usize);
        let flags = if parts.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS {
            WINHTTP_FLAG_SECURE
        } else {
            WINHTTP_OPEN_REQUEST_FLAGS(0)
        };
        // SAFETY: the connection is open, the verb and the object NUL-ended
        // wide strings; no version, referrer or accept types are given.
        let raw = unsafe {
            WinHttpOpenRequest(
                connect.0 as *mut core::ffi::c_void,
                PCWSTR(verb.as_ptr()),
                PCWSTR(object.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                flags,
            )
        };
        if raw.is_null() {
            return Err(wire::failure(last_error(), Stage::Open, 0, url));
        }
        let exchange = Arc::new(Exchange {
            gate: Gate::new(raw as usize, close),
            headers: wire::request_headers(request.headers()),
            body: request.body().to_vec(),
            buffer: ReadBuffer::new(),
        });
        let context = Arc::as_ptr(&exchange) as usize;
        if let Err(failed) = register(raw, context) {
            // No callback is registered, so none hears the close.
            close(raw as usize);
            return Err(wire::failure(failed, Stage::Open, 0, url));
        }
        // The callback holds this reference until the handle's last
        // notification gives it back.
        let _ = Arc::into_raw(Arc::clone(&exchange));
        Ok(Box::new(WinHttpConnection {
            exchange,
            url: url.to_owned(),
            _connect: connect,
        }))
    }
}

/// Points `request`'s notifications at the callback with `context`; the
/// error code when either step failed, after which no callback runs.
fn register(request: *mut core::ffi::c_void, context: usize) -> Result<(), u32> {
    // SAFETY: `request` is the open request handle; the option takes a
    // pointer-sized value.
    unsafe {
        WinHttpSetOption(
            Some(request.cast_const()),
            WINHTTP_OPTION_CONTEXT_VALUE,
            Some(&context.to_ne_bytes()),
        )
    }
    .map_err(|error| code(&error))?;
    // SAFETY: `request` is the open request handle and `status_callback`
    // has WinHTTP's callback signature.
    let previous =
        unsafe { WinHttpSetStatusCallback(request, Some(status_callback), NOTIFICATIONS, 0) };
    if previous.is_some_and(|previous| previous as usize == INVALID_STATUS_CALLBACK) {
        return Err(last_error());
    }
    Ok(())
}

/// What a request's thread, its callback and its aborter share. The
/// callback holds a reference until the handle's last notification, so
/// what WinHTTP reads (the headers, the body) and writes (the read buffer)
/// outlives every operation, an aborted one included.
struct Exchange {
    gate: Gate,
    headers: Option<Vec<u16>>,
    body: Vec<u8>,
    buffer: ReadBuffer,
}

/// The buffer WinHTTP reads the body into, as an address: WinHTTP writes it
/// while a read is pending, and only the request's thread reads it, after
/// the read completed.
struct ReadBuffer(usize);

impl ReadBuffer {
    fn new() -> Self {
        let buffer: Box<[u8; READ_BUFFER]> = Box::new([0; READ_BUFFER]);
        Self(Box::into_raw(buffer) as usize)
    }

    fn as_mut_ptr(&self) -> *mut u8 {
        self.0 as *mut u8
    }
}

impl Drop for ReadBuffer {
    fn drop(&mut self) {
        // SAFETY: made by `Box::into_raw` in `new` and freed only here; the
        // exchange that owns it drops after the handle's last notification,
        // so WinHTTP no longer writes it.
        drop(unsafe { Box::from_raw(self.0 as *mut [u8; READ_BUFFER]) });
    }
}

/// Aborts a request from any thread through its gate.
struct Aborter(Arc<Exchange>);

impl Abort for Aborter {
    fn abort(&self) {
        self.0.gate.abort();
    }
}

/// One request, used from its own thread.
struct WinHttpConnection {
    exchange: Arc<Exchange>,
    url: String,
    /// Closed after the request handle, which keeps it alive until then.
    _connect: Handle,
}

impl WinHttpConnection {
    fn ended(&self, stage: Stage, outcome: Outcome) -> Result<u32, HttpError> {
        match outcome {
            Outcome::Completed(moved) => Ok(moved),
            Outcome::Failed(code) => Err(wire::failure(
                code,
                stage,
                self.exchange.gate.secure_failure(),
                &self.url,
            )),
            Outcome::Aborted => Err(HttpError::Cancelled),
        }
    }

    fn status(&self) -> Result<StatusCode, HttpError> {
        let queried = self.exchange.gate.call(|handle| {
            let mut status = 0_u32;
            let mut size = u32::try_from(size_of::<u32>()).expect("BUG: a u32 is 4 bytes");
            // SAFETY: the handle is open and the head arrived; the buffer is
            // a `u32` of `size` bytes, as the number flag asks.
            unsafe {
                WinHttpQueryHeaders(
                    handle as *mut core::ffi::c_void,
                    WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                    PCWSTR::null(),
                    Some((&raw mut status).cast()),
                    &raw mut size,
                    std::ptr::null_mut(),
                )
            }
            .map(|()| status)
            .map_err(|error| code(&error))
        });
        let status = self.queried(queried)?;
        StatusCode::try_new(u16::try_from(status).map_err(|_| HttpError::Protocol {
            detail: format!("status {status}"),
        })?)
    }

    fn raw_headers(&self) -> Result<Vec<u16>, HttpError> {
        let queried = self.exchange.gate.call(|handle| {
            let query = |buffer: &mut [u16], size: &mut u32| {
                let into = (!buffer.is_empty()).then_some(buffer.as_mut_ptr().cast());
                // SAFETY: the handle is open and the head arrived; `into` is
                // `size` bytes of `buffer`, or none to learn the size.
                unsafe {
                    WinHttpQueryHeaders(
                        handle as *mut core::ffi::c_void,
                        WINHTTP_QUERY_RAW_HEADERS_CRLF,
                        PCWSTR::null(),
                        into,
                        size,
                        std::ptr::null_mut(),
                    )
                }
                .map_err(|error| code(&error))
            };
            let mut size = 0_u32;
            match query(&mut [], &mut size) {
                Err(failed) if failed == ERROR_INSUFFICIENT_BUFFER.0 => {}
                Err(failed) => return Err(failed),
                Ok(()) => return Ok(Vec::new()),
            }
            let mut buffer = vec![0_u16; (size as usize).div_ceil(2)];
            query(&mut buffer, &mut size)?;
            buffer.truncate(size as usize / 2);
            Ok(buffer)
        });
        self.queried(queried)
    }

    fn queried<T>(&self, queried: Option<Result<T, u32>>) -> Result<T, HttpError> {
        match queried {
            Some(Ok(answer)) => Ok(answer),
            Some(Err(failed)) => Err(wire::failure(failed, Stage::Send, 0, &self.url)),
            None => Err(HttpError::Cancelled),
        }
    }
}

impl Connection for WinHttpConnection {
    fn aborter(&self) -> Arc<dyn Abort> {
        Arc::new(Aborter(Arc::clone(&self.exchange)))
    }

    fn send(&mut self) -> Result<Head, HttpError> {
        let exchange = Arc::clone(&self.exchange);
        let length = u32::try_from(exchange.body.len()).map_err(|_| HttpError::Host {
            detail: "WinHTTP sends a body of at most 4 GiB".to_owned(),
        })?;
        let context = Arc::as_ptr(&exchange) as usize;
        let sent = exchange.gate.run(|handle| {
            let body = (length > 0).then_some(exchange.body.as_ptr().cast());
            // SAFETY: the handle is open; the headers and the body live in
            // the exchange, which the callback keeps past the send's end.
            unsafe {
                WinHttpSendRequest(
                    handle as *mut core::ffi::c_void,
                    exchange.headers.as_deref(),
                    body,
                    length,
                    length,
                    context,
                )
            }
            .map_err(|error| code(&error))
        });
        self.ended(Stage::Send, sent)?;
        let received = exchange.gate.run(|handle| {
            // SAFETY: the handle is open and its request was sent.
            unsafe {
                WinHttpReceiveResponse(handle as *mut core::ffi::c_void, std::ptr::null_mut())
            }
            .map_err(|error| code(&error))
        });
        self.ended(Stage::Send, received)?;
        Ok(Head {
            status: self.status()?,
            headers: wire::response_headers(&self.raw_headers()?)?,
        })
    }

    fn read(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        let buffer = self.exchange.buffer.as_mut_ptr();
        let read = self.exchange.gate.run(|handle| {
            // SAFETY: the handle is open and its head arrived; the buffer
            // lives in the exchange, which the callback keeps until WinHTTP
            // is done with it; the asynchronous read reports its length to
            // the callback, so none is asked for here.
            unsafe {
                WinHttpReadData(
                    handle as *mut core::ffi::c_void,
                    buffer.cast(),
                    u32::try_from(READ_BUFFER).expect("BUG: the read buffer is 16 KiB"),
                    std::ptr::null_mut(),
                )
            }
            .map_err(|error| code(&error))
        });
        let read = self.ended(Stage::Read, read)? as usize;
        if read == 0 {
            return Ok(None);
        }
        // SAFETY: the read completed, so WinHTTP wrote `read` bytes of the
        // buffer (at most its size) and holds it no longer; nothing else
        // touches it until the next read starts.
        Ok(Some(
            unsafe { std::slice::from_raw_parts(buffer, read.min(READ_BUFFER)) }.to_vec(),
        ))
    }
}

impl Drop for WinHttpConnection {
    fn drop(&mut self) {
        self.exchange.gate.abort();
    }
}

/// WinHTTP's status callback for every request.
unsafe extern "system" fn status_callback(
    _handle: *mut core::ffi::c_void,
    context: usize,
    status: u32,
    info: *mut core::ffi::c_void,
    length: u32,
) {
    os_entry(
        "the WinHTTP status callback",
        (context, status, info as usize, length),
        AfterPanic::Answer(|_| ()),
        || {
            // SAFETY: WinHTTP calls with the context `open` registered and
            // the information `status` carries.
            unsafe { deliver(context, status, info, length) }
        },
    );
}

/// Hands a notification to its request's gate.
///
/// # Safety
///
/// `context` is 0 or the address of the [`Exchange`] whose reference `open`
/// gave the callback, not yet given back; `info` points to what `status`
/// carries.
unsafe fn deliver(context: usize, status: u32, info: *mut core::ffi::c_void, length: u32) {
    if context == 0 {
        return;
    }
    let exchange = context as *const Exchange;
    if status == WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING {
        // SAFETY: the handle's last notification: the reference `open` gave
        // the callback comes back here, once.
        drop(unsafe { Arc::from_raw(exchange) });
        return;
    }
    // SAFETY: the callback's reference keeps the exchange alive until the
    // handle's last notification.
    let gate = unsafe { &(*exchange).gate };
    match status {
        WINHTTP_CALLBACK_STATUS_SENDREQUEST_COMPLETE
        | WINHTTP_CALLBACK_STATUS_HEADERS_AVAILABLE => {
            gate.complete(Ok(0));
        }
        WINHTTP_CALLBACK_STATUS_READ_COMPLETE => gate.complete(Ok(length)),
        WINHTTP_CALLBACK_STATUS_REQUEST_ERROR => {
            // SAFETY: a request error carries a `WINHTTP_ASYNC_RESULT`.
            let result = unsafe { info.cast::<WINHTTP_ASYNC_RESULT>().read_unaligned() };
            gate.complete(Err(result.dwError));
        }
        WINHTTP_CALLBACK_STATUS_SECURE_FAILURE => {
            // SAFETY: a secure failure carries its flags as a `u32`.
            let flags = unsafe { info.cast::<u32>().read_unaligned() };
            gate.note_secure_failure(flags);
        }
        _ => {}
    }
}
