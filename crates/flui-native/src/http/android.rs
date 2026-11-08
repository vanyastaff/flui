//! [`UrlConnectionClient`]: HTTP through Android's `HttpURLConnection`.
//!
//! Each request runs on a thread of its own (the `blocking` module's model), attached
//! to the VM for its life; only system classes are called, so no class of
//! this library's dex is needed and nothing calls back into Rust. The system
//! stack brings the device's proxy settings and its trust store, the
//! certificates a user or a device policy installed included.
//!
//! Every JNI failure becomes an [`HttpError`]: a Java exception by its class
//! (a TLS one is [`HttpError::Tls`], a socket timeout [`HttpError::Timeout`],
//! a failed lookup or connection [`HttpError::Connect`]), and a failure of
//! the bridge itself [`HttpError::Host`]. Nothing panics on the request's
//! thread.

use std::sync::Arc;

use flui_platform::http::{
    Headers, HttpAuthority, HttpClient, HttpError, HttpFuture, HttpRequest, HttpResponse,
    StatusCode,
};
use jni::objects::{JByteArray, JObject, JString};
use jni::refs::Global;
use jni::{Env, JValue, JavaVM, jni_sig, jni_str};

use super::blocking::{Abort, BlockingClient, Connection, Head, Stack};
use super::head::HeadBuilder;

/// How long a connection may take to open, and a read to answer.
const SOCKET_TIMEOUT_MS: i32 = 30_000;

/// The size of each body read.
const READ_BUFFER: usize = 16 * 1024;

/// An HTTP client over Android's `HttpURLConnection` (ADR-0280).
///
/// Each request runs on a thread of its own and answers through a channel,
/// so the future [`send`](HttpClient::send) returns, and the response's body,
/// are polled on any thread with no runtime of the caller's. The body is read
/// a chunk at a time, only as fast as it is read. Dropping the request or its
/// body disconnects it, and the request's deadline disconnects it with
/// [`HttpError::Timeout`].
pub struct UrlConnectionClient(BlockingClient<UrlConnections>);

impl UrlConnectionClient {
    /// A client over the system's `HttpURLConnection`.
    #[must_use]
    pub fn new() -> Self {
        Self(BlockingClient::new(UrlConnections))
    }
}

impl Default for UrlConnectionClient {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for UrlConnectionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UrlConnectionClient")
            .finish_non_exhaustive()
    }
}

impl HttpClient for UrlConnectionClient {
    fn authority(&self) -> HttpAuthority {
        self.0.authority()
    }

    fn send(&self, request: HttpRequest) -> HttpFuture<HttpResponse> {
        self.0.send(request)
    }
}

/// Where in the exchange a Java exception was thrown: what it means when its
/// class says nothing more.
#[derive(Clone, Copy)]
enum Stage {
    Open,
    Send,
    Read,
}

/// The [`HttpError`] a JNI failure at `stage` stands for.
fn failure(error: jni::errors::Error, stage: Stage, url: &str) -> HttpError {
    let jni::errors::Error::CaughtJavaException { name, msg, .. } = error else {
        return HttpError::Host {
            detail: format!("JNI: {error}"),
        };
    };
    let detail = format!("{name}: {msg}");
    match name.as_str() {
        "java.net.MalformedURLException" => HttpError::InvalidUrl {
            url: url.to_owned(),
        },
        "java.net.SocketTimeoutException" => HttpError::Timeout,
        "java.net.UnknownHostException"
        | "java.net.ConnectException"
        | "java.net.NoRouteToHostException"
        | "java.net.PortUnreachableException" => HttpError::Connect { detail },
        tls if tls.starts_with("javax.net.ssl.") => HttpError::Tls { detail },
        _ => match stage {
            Stage::Open => HttpError::Host { detail },
            Stage::Send => HttpError::Connect { detail },
            Stage::Read => HttpError::Protocol { detail },
        },
    }
}

/// The VM this process runs, once the Android host started.
fn vm() -> Result<JavaVM, HttpError> {
    JavaVM::singleton().map_err(|error| HttpError::Host {
        detail: format!("no Java VM: {error}"),
    })
}

/// The system's `HttpURLConnection`.
struct UrlConnections;

impl Stack for UrlConnections {
    fn open(&self, request: &HttpRequest) -> Result<Box<dyn Connection>, HttpError> {
        let url = request.url();
        let scheme = url.split_once("://").map(|(scheme, _)| scheme);
        if !scheme.is_some_and(|scheme| {
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
        }) {
            return Err(HttpError::InvalidUrl {
                url: url.to_owned(),
            });
        }
        let vm = vm()?;
        let connection = vm
            .attach_current_thread(|env| open(env, request))
            .map_err(|error| failure(error, Stage::Open, url))?;
        Ok(Box::new(UrlConnection {
            vm,
            connection: Arc::new(connection),
            request: request.clone(),
            stream: None,
            buffer: None,
            drained: false,
        }))
    }
}

/// `new URL(url).openConnection()`, set up for `request` but not connected.
fn open(env: &mut Env<'_>, request: &HttpRequest) -> jni::errors::Result<Global<JObject<'static>>> {
    let url = env.new_string(request.url())?;
    let url = env.new_object(
        jni_str!("java/net/URL"),
        jni_sig!("(Ljava/lang/String;)V"),
        &[JValue::Object(&url)],
    )?;
    let connection = env
        .call_method(
            &url,
            jni_str!("openConnection"),
            jni_sig!("()Ljava/net/URLConnection;"),
            &[],
        )?
        .l()?;
    let method = env.new_string(request.method().as_str())?;
    env.call_method(
        &connection,
        jni_str!("setRequestMethod"),
        jni_sig!("(Ljava/lang/String;)V"),
        &[JValue::Object(&method)],
    )?
    .v()?;
    for (setter, value) in [
        (jni_str!("setConnectTimeout"), SOCKET_TIMEOUT_MS),
        (jni_str!("setReadTimeout"), SOCKET_TIMEOUT_MS),
    ] {
        env.call_method(&connection, setter, jni_sig!("(I)V"), &[JValue::Int(value)])?
            .v()?;
    }
    for (name, value) in request.headers().iter() {
        let name = env.new_string(name.as_str())?;
        // A header value is a byte string: one character per byte.
        let value: String = value.as_bytes().iter().copied().map(char::from).collect();
        let value = env.new_string(value)?;
        env.call_method(
            &connection,
            jni_str!("addRequestProperty"),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;)V"),
            &[JValue::Object(&name), JValue::Object(&value)],
        )?
        .v()?;
    }
    env.new_global_ref(&connection)
}

/// One request's `HttpURLConnection`, used from the request's thread.
struct UrlConnection {
    vm: JavaVM,
    connection: Arc<Global<JObject<'static>>>,
    request: HttpRequest,
    /// The body's stream once the head arrived, unless the response has none.
    stream: Option<Global<JObject<'static>>>,
    /// The array each read lands in.
    buffer: Option<Global<JByteArray<'static>>>,
    /// The body was read to its end: the connection may go back to the
    /// system's pool.
    drained: bool,
}

impl Connection for UrlConnection {
    fn aborter(&self) -> Arc<dyn Abort> {
        Arc::new(Disconnect {
            vm: self.vm.clone(),
            connection: Arc::clone(&self.connection),
        })
    }

    fn send(&mut self) -> Result<Head, HttpError> {
        let connection = Arc::clone(&self.connection);
        let request = &self.request;
        let Answered { head, stream } = self
            .vm
            .attach_current_thread(|env| send(env, &connection, request))
            .map_err(|error| failure(error, Stage::Send, request.url()))?;
        self.stream = stream;
        head
    }

    fn read(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        let Some(stream) = &self.stream else {
            self.drained = true;
            return Ok(None);
        };
        let url = self.request.url();
        let buffer = &mut self.buffer;
        let read = self
            .vm
            .attach_current_thread(|env| -> jni::errors::Result<Option<Vec<u8>>> {
                if buffer.is_none() {
                    let array = env.new_byte_array(READ_BUFFER)?;
                    *buffer = Some(env.new_global_ref(&array)?);
                }
                let array = buffer.as_ref().expect("BUG: the read buffer was just made");
                let read = env
                    .call_method(
                        stream,
                        jni_str!("read"),
                        jni_sig!("([B)I"),
                        &[JValue::Object(array.as_ref())],
                    )?
                    .i()?;
                // `-1` at the end; never more than the array holds.
                let Ok(read) = usize::try_from(read) else {
                    return Ok(None);
                };
                let mut chunk = vec![0_i8; read.min(READ_BUFFER)];
                array.get_region(env, 0, &mut chunk)?;
                Ok(Some(chunk.into_iter().map(i8::cast_unsigned).collect()))
            })
            .map_err(|error| failure(error, Stage::Read, url));
        self.drained = matches!(read, Ok(None));
        read
    }
}

impl Drop for UrlConnection {
    fn drop(&mut self) {
        // A body read to its end is closed, which hands a keep-alive socket
        // back to the system's pool; anything else lets the socket go.
        if self.drained {
            let Some(stream) = &self.stream else {
                return;
            };
            let closed = self.vm.attach_current_thread(|env| {
                env.call_method(stream, jni_str!("close"), jni_sig!("()V"), &[])?
                    .v()
            });
            if closed.is_ok() {
                return;
            }
        }
        Disconnect {
            vm: self.vm.clone(),
            connection: Arc::clone(&self.connection),
        }
        .abort();
    }
}

/// What sending a request answered: its head, or the status and headers the
/// contract refuses, and the body's stream unless the response has none.
struct Answered {
    head: Result<Head, HttpError>,
    stream: Option<Global<JObject<'static>>>,
}

/// Sends the request on `connection` and reads its head: the head, or a
/// status and headers the contract refuses, and the body's stream.
fn send(
    env: &mut Env<'_>,
    connection: &Global<JObject<'static>>,
    request: &HttpRequest,
) -> jni::errors::Result<Answered> {
    if !request.body().is_empty() {
        env.call_method(
            connection,
            jni_str!("setDoOutput"),
            jni_sig!("(Z)V"),
            &[JValue::Bool(true)],
        )?
        .v()?;
        let length = i64::try_from(request.body().len()).unwrap_or(i64::MAX);
        env.call_method(
            connection,
            jni_str!("setFixedLengthStreamingMode"),
            jni_sig!("(J)V"),
            &[JValue::Long(length)],
        )?
        .v()?;
        let output = env
            .call_method(
                connection,
                jni_str!("getOutputStream"),
                jni_sig!("()Ljava/io/OutputStream;"),
                &[],
            )?
            .l()?;
        let body = env.byte_array_from_slice(request.body())?;
        env.call_method(
            &output,
            jni_str!("write"),
            jni_sig!("([B)V"),
            &[JValue::Object(&body)],
        )?
        .v()?;
        env.call_method(&output, jni_str!("close"), jni_sig!("()V"), &[])?
            .v()?;
    }
    let code = env
        .call_method(
            connection,
            jni_str!("getResponseCode"),
            jni_sig!("()I"),
            &[],
        )?
        .i()?;
    let headers = read_headers(env, connection)?;
    // A 4xx or 5xx body is the error stream; either may be null.
    let getter = if code >= 400 {
        jni_str!("getErrorStream")
    } else {
        jni_str!("getInputStream")
    };
    let stream = env
        .call_method(connection, getter, jni_sig!("()Ljava/io/InputStream;"), &[])?
        .l()?;
    let stream = if stream.is_null() {
        None
    } else {
        Some(env.new_global_ref(&stream)?)
    };
    let head = u16::try_from(code)
        .map_err(|_| HttpError::Protocol {
            detail: format!("status code {code}"),
        })
        .and_then(StatusCode::try_new)
        .and_then(|status| {
            Ok(Head {
                status,
                headers: headers?,
            })
        });
    Ok(Answered { head, stream })
}

/// The response's headers, in order; the status line (a null key) left out.
fn read_headers(
    env: &mut Env<'_>,
    connection: &Global<JObject<'static>>,
) -> jni::errors::Result<Result<Headers, HttpError>> {
    let mut headers = HeadBuilder::new();
    let mut index = 0;
    loop {
        let key = env
            .call_method(
                connection,
                jni_str!("getHeaderFieldKey"),
                jni_sig!("(I)Ljava/lang/String;"),
                &[JValue::Int(index)],
            )?
            .l()?;
        let value = env
            .call_method(
                connection,
                jni_str!("getHeaderField"),
                jni_sig!("(I)Ljava/lang/String;"),
                &[JValue::Int(index)],
            )?
            .l()?;
        index += 1;
        if value.is_null() {
            return Ok(Ok(headers.finish()));
        }
        if key.is_null() {
            continue;
        }
        let key = env.cast_local::<JString<'_>>(key)?.try_to_string(env)?;
        let value = env.cast_local::<JString<'_>>(value)?.try_to_string(env)?;
        if let Err(error) = headers.try_push_byte_string(&key, &value) {
            return Ok(Err(error));
        }
    }
}

/// `HttpURLConnection.disconnect()` from any thread: a blocked call on the
/// connection fails at once.
struct Disconnect {
    vm: JavaVM,
    connection: Arc<Global<JObject<'static>>>,
}

impl Abort for Disconnect {
    fn abort(&self) {
        let disconnected = self.vm.attach_current_thread(|env| {
            env.call_method(
                self.connection.as_ref(),
                jni_str!("disconnect"),
                jni_sig!("()V"),
                &[],
            )?
            .v()
        });
        if let Err(error) = disconnected {
            tracing::debug!(
                target: "flui_native::android",
                %error,
                "disconnecting an HTTP request failed"
            );
        }
    }
}
