//! The client, its `NSURLSession`, and the delegate that hands each task's
//! response, data and end to the task's [`Transfer`].

// This module subclasses `NSObject` and hands the subclass to the session,
// which has no safe wrapper: the workspace lint `unsafe_code = "warn"` is
// opted out here, at the module boundary, rather than for the whole crate (see
// `lib.rs`).
#![expect(
    unsafe_code,
    reason = "subclassing `NSObject` and handing the subclass to the session has no safe wrapper"
)]

use rustc_hash::FxHashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use flui_platform::http::{
    HeaderName, HeaderValue, Headers, HttpAuthority, HttpClient, HttpError, HttpFuture,
    HttpRequest, HttpResponse, StatusCode,
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{
    NSData, NSError, NSHTTPURLResponse, NSMutableURLRequest, NSObject, NSObjectProtocol, NSString,
    NSURL, NSURLResponse, NSURLSession, NSURLSessionConfiguration, NSURLSessionDataDelegate,
    NSURLSessionDataTask, NSURLSessionDelegate, NSURLSessionResponseDisposition, NSURLSessionTask,
    NSURLSessionTaskDelegate,
};

use super::transfer::{Disposition, Transfer};
use crate::http::blocking::{Abort, BlockingClient, Connection, Head, Stack};
use crate::owner::panic_boundary::{AfterPanic, ContainedIvars, os_entry};

/// How long a request may wait for its next bytes, in seconds.
const IDLE_TIMEOUT_SECS: f64 = 30.0;

/// The most response headers a head may carry.
const MAX_HEADERS: usize = 1024;

/// An HTTP client over Apple's `NSURLSession` (ADR-0280).
///
/// Each request runs on a thread of its own and answers through a channel,
/// so the future [`send`](HttpClient::send) returns, and the response's body,
/// are polled on any thread with no runtime of the caller's. The body is read
/// only as fast as it is read: the task is suspended while the reader holds
/// what arrived. Dropping the request or its body cancels its task, and the
/// request's deadline cancels it with [`HttpError::Timeout`]. Requests of one
/// client share one session and its connections.
pub struct UrlSessionClient(BlockingClient<Session>);

impl UrlSessionClient {
    /// A client over an ephemeral `NSURLSession` of its own.
    #[must_use]
    pub fn new() -> Self {
        Self(BlockingClient::new(Session::new()))
    }
}

impl Default for UrlSessionClient {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for UrlSessionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UrlSessionClient").finish_non_exhaustive()
    }
}

impl HttpClient for UrlSessionClient {
    fn authority(&self) -> HttpAuthority {
        self.0.authority()
    }

    fn send(&self, request: HttpRequest) -> HttpFuture<HttpResponse> {
        self.0.send(request)
    }
}

/// The client's session and the delegate that routes each task's events.
struct Session {
    session: Retained<NSURLSession>,
    delegate: Retained<SessionDelegate>,
}

impl Session {
    fn new() -> Self {
        let configuration = NSURLSessionConfiguration::ephemeralSessionConfiguration();
        configuration.setTimeoutIntervalForRequest(IDLE_TIMEOUT_SECS);
        let delegate = SessionDelegate::new();
        // SAFETY: the delegate's class implements the protocol's methods with
        // their declared signatures; a `None` queue makes the session a
        // serial queue of its own for them.
        let session = unsafe {
            NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                &configuration,
                Some(ProtocolObject::from_ref(&*delegate)),
                None,
            )
        };
        Self { session, delegate }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // The session holds its delegate until it is invalidated. The last
        // request's thread has left, so no task is left to cancel.
        self.session.invalidateAndCancel();
    }
}

impl Stack for Session {
    fn open(&self, request: &HttpRequest) -> Result<Box<dyn Connection>, HttpError> {
        let invalid = || HttpError::InvalidUrl {
            url: request.url.clone(),
        };
        let scheme = request.url.split_once("://").map(|(scheme, _)| scheme);
        if !scheme.is_some_and(|scheme| {
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
        }) {
            return Err(invalid());
        }
        let url =
            NSURL::URLWithString(&NSString::from_str(request.url.as_str())).ok_or_else(invalid)?;
        let native = NSMutableURLRequest::requestWithURL(&url);
        native.setHTTPMethod(&NSString::from_str(request.method.as_str()));
        native.setTimeoutInterval(IDLE_TIMEOUT_SECS);
        for (name, value) in request.headers.iter() {
            // A header value is a byte string: one character per byte.
            let value: String = value.as_bytes().iter().copied().map(char::from).collect();
            native.addValue_forHTTPHeaderField(
                &NSString::from_str(&value),
                &NSString::from_str(name.as_str()),
            );
        }
        if !request.body.is_empty() {
            native.setHTTPBody(Some(&NSData::with_bytes(&request.body)));
        }
        let task = self.session.dataTaskWithRequest(&native);
        let transfer = Arc::new(Transfer::new(request.url.clone()));
        let id = task.taskIdentifier();
        self.delegate
            .ivars()
            .lock()
            .insert(id, Arc::clone(&transfer));
        Ok(Box::new(SessionConnection {
            task,
            id,
            transfer,
            delegate: self.delegate.clone(),
        }))
    }
}

/// One request's task, used from the request's thread.
struct SessionConnection {
    task: Retained<NSURLSessionDataTask>,
    id: usize,
    transfer: Arc<Transfer>,
    delegate: Retained<SessionDelegate>,
}

impl Connection for SessionConnection {
    fn aborter(&self) -> Arc<dyn Abort> {
        Arc::new(Cancel {
            task: self.task.clone(),
            transfer: Arc::clone(&self.transfer),
        })
    }

    fn send(&mut self) -> Result<Head, HttpError> {
        self.task.resume();
        self.transfer.await_head()
    }

    fn read(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        self.transfer.await_chunk(|| self.task.resume())
    }
}

impl Drop for SessionConnection {
    fn drop(&mut self) {
        // Harmless on a completed task; a running one stops delivering.
        self.task.cancel();
        self.delegate.ivars().lock().remove(&self.id);
    }
}

/// Cancels a task from any thread, and wakes its request's thread at once
/// rather than when the delegate hears of it.
struct Cancel {
    task: Retained<NSURLSessionDataTask>,
    transfer: Arc<Transfer>,
}

impl Abort for Cancel {
    fn abort(&self) {
        self.transfer.end(Err(HttpError::Cancelled));
        self.task.cancel();
    }
}

/// The tasks a session's delegate routes events to, by task identifier.
#[derive(Default)]
struct Tasks(Mutex<FxHashMap<usize, Arc<Transfer>>>);

impl Tasks {
    fn lock(&self) -> MutexGuard<'_, FxHashMap<usize, Arc<Transfer>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn get(&self, task: &NSURLSessionTask) -> Option<Arc<Transfer>> {
        self.lock().get(&task.taskIdentifier()).cloned()
    }
}

define_class!(
    // SAFETY: NSObject has no additional subclass invariants; the ivars are
    // Rust-owned and `Sync`, so the class may be used from the session's
    // queue and the request threads alike.
    #[unsafe(super = NSObject)]
    #[ivars = ContainedIvars<Tasks>]
    /// A session's delegate: hands each task's response, data and end to
    /// the request thread waiting for it.
    struct SessionDelegate;

    // SAFETY: NSObjectProtocol has no additional requirements.
    unsafe impl NSObjectProtocol for SessionDelegate {}

    // SAFETY: no method of the protocol is implemented.
    unsafe impl NSURLSessionDelegate for SessionDelegate {}

    // SAFETY: the method matches the protocol's declaration.
    unsafe impl NSURLSessionTaskDelegate for SessionDelegate {
        #[unsafe(method(URLSession:task:didCompleteWithError:))]
        fn did_complete(
            &self,
            _session: &NSURLSession,
            task: &NSURLSessionTask,
            error: Option<&NSError>,
        ) {
            os_entry(
                "URLSession:task:didCompleteWithError:",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    let Some(transfer) = self.ivars().get(task) else {
                        return;
                    };
                    transfer.completed(|headed| {
                        error.map(|error| failure(error, headed, transfer.url()))
                    });
                },
            );
        }
    }

    // SAFETY: the methods match the protocol's declarations.
    unsafe impl NSURLSessionDataDelegate for SessionDelegate {
        #[unsafe(method(URLSession:dataTask:didReceiveResponse:completionHandler:))]
        fn did_receive_response(
            &self,
            _session: &NSURLSession,
            task: &NSURLSessionDataTask,
            response: &NSURLResponse,
            completion_handler: &block2::DynBlock<dyn Fn(NSURLSessionResponseDisposition)>,
        ) {
            // The system's block is called once, with no lock held: with the
            // disposition, or after a panic before it, with a cancel.
            os_entry(
                "URLSession:dataTask:didReceiveResponse:completionHandler:",
                completion_handler,
                AfterPanic::Answer(|handler| {
                    handler.call((NSURLSessionResponseDisposition::Cancel,));
                }),
                || {
                    let disposition = self.ivars().get(task).map_or(
                        NSURLSessionResponseDisposition::Cancel,
                        |transfer| match transfer.responded(|| head(response)) {
                            Disposition::Allow => NSURLSessionResponseDisposition::Allow,
                            Disposition::Cancel => NSURLSessionResponseDisposition::Cancel,
                        },
                    );
                    completion_handler.call((disposition,));
                },
            );
        }

        #[unsafe(method(URLSession:dataTask:didReceiveData:))]
        fn did_receive_data(
            &self,
            _session: &NSURLSession,
            task: &NSURLSessionDataTask,
            data: &NSData,
        ) {
            os_entry(
                "URLSession:dataTask:didReceiveData:",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    let Some(transfer) = self.ivars().get(task) else {
                        return;
                    };
                    let disposition = transfer.received(|| data.to_vec(), || task.suspend());
                    if disposition == Disposition::Cancel {
                        // The body lost bytes and its transfer has ended.
                        task.cancel();
                    }
                },
            );
        }
    }
);

impl SessionDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(ContainedIvars::new(Tasks::default()));
        // SAFETY: NSObject's `init` on a freshly allocated instance.
        unsafe { msg_send![super(this), init] }
    }
}

/// The head of `response`, or why the contract refuses it.
fn head(response: &NSURLResponse) -> Result<Head, HttpError> {
    let Some(response) = response.downcast_ref::<NSHTTPURLResponse>() else {
        return Err(protocol("the response is not HTTP"));
    };
    let status = u16::try_from(response.statusCode())
        .map_err(|_| protocol(format!("status code {}", response.statusCode())))
        .and_then(StatusCode::try_new)?;
    let fields = response.allHeaderFields();
    let keys = fields.allKeys();
    if keys.len() > MAX_HEADERS {
        return Err(protocol(format!("more than {MAX_HEADERS} headers")));
    }
    let mut headers = Headers::new();
    for key in &keys {
        let (Some(name), Some(value)) = (
            key.downcast_ref::<NSString>(),
            fields
                .objectForKey(&key)
                .and_then(|value| value.downcast::<NSString>().ok()),
        ) else {
            return Err(protocol("a header is not a string"));
        };
        let name = HeaderName::try_new(&name.to_string()).map_err(protocol)?;
        let value = HeaderValue::try_new(value.to_string()).map_err(protocol)?;
        headers.append(name, value);
    }
    Ok(Head { status, headers })
}

/// The [`HttpError`] an `NSError` stands for: by its code in
/// `NSURLErrorDomain`, else by whether the head had arrived.
fn failure(error: &NSError, headed: bool, url: &str) -> HttpError {
    let detail = error.localizedDescription().to_string();
    let url_error = error.domain().to_string() == "NSURLErrorDomain";
    match error.code() {
        // NSURLErrorTimedOut
        -1001 if url_error => HttpError::Timeout,
        // NSURLErrorCancelled: only this client cancels its tasks.
        -999 if url_error => HttpError::Cancelled,
        // NSURLErrorBadURL, NSURLErrorUnsupportedURL
        -1000 | -1002 if url_error => HttpError::InvalidUrl {
            url: url.to_owned(),
        },
        // Cannot find or connect to the host, the connection was lost, no
        // network, DNS, roaming or data not allowed.
        -1003 | -1004 | -1005 | -1006 | -1009 | -1018 | -1019 | -1020 if url_error => {
            HttpError::Connect { detail }
        }
        // The secure connection failed, a server certificate is bad,
        // untrusted, expired or not yet valid, or a client certificate was
        // refused or required.
        -1206..=-1200 if url_error => HttpError::Tls { detail },
        _ if headed => HttpError::Protocol { detail },
        _ => HttpError::Connect { detail },
    }
}

fn protocol(error: impl std::fmt::Display) -> HttpError {
    HttpError::Protocol {
        detail: error.to_string(),
    }
}
