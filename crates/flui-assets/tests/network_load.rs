//! Network image loads: fetched with the caller's HTTP client, on the lanes a
//! load is given or on none, and the typed errors of a failed fetch.

use std::time::{Duration, Instant};

use flui_assets::{AssetError, AssetRegistryBuilder, LoadServices};
use flui_platform::http::{HttpError, StatusCode};
use flui_platform::testing::{ScriptedHttp, ScriptedResponse};

use crate::lanes::{QueuedLanes, ThreadLanes, block_on};

const PNG: &[u8] = include_bytes!("fixtures/tiny.png");
const URL: &str = "https://example.test/image.png";

#[test]
fn network_loads_fetch_with_the_callers_client_and_recover_after_errors() {
    crate::cases::run_cases(&[
        ("decode error, then recovery", decode_error_then_recovery),
        ("no lanes, no runtime", no_lanes),
        ("error status", error_status),
        ("failed request", failed_request),
        ("no client", no_client),
        ("handed over to the next client", handed_over),
    ]);
}

/// A failed decode is not kept, so the same URL is fetched again; each
/// request carries the registry's body limit.
fn decode_error_then_recovery() {
    let http = ScriptedHttp::new();
    http.respond(URL, ScriptedResponse::ok(b"invalid image".to_vec()));
    let lanes = ThreadLanes::new();
    let services = lanes.services().with_http(http.handle());
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let malformed = block_on(registry.load_network_image(URL, services.clone()));
    assert!(
        matches!(
            &malformed,
            Err(AssetError::LoadFailed { .. } | AssetError::DecodeFailed { .. })
        ),
        "a malformed image fails decoding, got {malformed:?}"
    );
    http.respond(URL, ScriptedResponse::ok(PNG.to_vec()));
    let image = block_on(registry.load_network_image(URL, services.clone()))
        .expect("the next load recovers");
    assert_eq!((image.width(), image.height()), (4, 2));
    assert_eq!(http.requests_to(URL), 2, "the failure was not kept");
    let _cached = block_on(registry.load_network_image(URL, services)).expect("a cache hit");
    assert_eq!(
        http.requests_to(URL),
        2,
        "a decoded image is served without HTTP"
    );
    let limit = http.requests()[0].body_limit();
    assert_eq!(limit, 256 * 1024 * 1024, "the registry bounds the body");
}

/// With no lanes the load runs where it is polled, and the client needs no
/// async runtime there: a headless host loads network images.
///
/// Red-check: put back a Tokio reactor check before the request.
fn no_lanes() {
    let http = ScriptedHttp::new();
    http.respond(URL, ScriptedResponse::ok(PNG.to_vec()));
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let image =
        block_on(registry.load_network_image(URL, LoadServices::new().with_http(http.handle())))
            .expect("the image loads with no lanes and no runtime");
    assert_eq!((image.width(), image.height()), (4, 2));
}

fn error_status() {
    let http = ScriptedHttp::new();
    http.respond(
        URL,
        ScriptedResponse::Respond {
            status: StatusCode::NOT_FOUND,
            body: b"not here".to_vec(),
        },
    );
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let answered =
        block_on(registry.load_network_image(URL, LoadServices::new().with_http(http.handle())));
    assert!(
        matches!(&answered, Err(AssetError::HttpStatus { status, .. }) if *status == StatusCode::NOT_FOUND),
        "{answered:?}"
    );
}

fn failed_request() {
    let http = ScriptedHttp::new();
    http.respond(URL, ScriptedResponse::Fail(HttpError::Timeout));
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let answered =
        block_on(registry.load_network_image(URL, LoadServices::new().with_http(http.handle())));
    assert!(
        matches!(
            &answered,
            Err(AssetError::Http {
                source: HttpError::Timeout,
                ..
            })
        ),
        "{answered:?}"
    );
}

/// A host with no HTTP client answers a typed error and sends nothing.
fn no_client() {
    let lanes = QueuedLanes::new();
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let pending = registry.load_network_image(URL, lanes.services());
    lanes.run_until_idle();
    let answered = block_on(pending);
    assert!(
        matches!(&answered, Err(AssetError::HttpUnavailable { url }) if url == URL),
        "{answered:?}"
    );
}

/// The lead's lanes shut down under its held request; the request waiting on
/// other lanes takes the load over and fetches with its own client.
fn handed_over() {
    let first_http = ScriptedHttp::new();
    first_http.hold(URL);
    let second_http = ScriptedHttp::new();
    second_http.respond(URL, ScriptedResponse::ok(PNG.to_vec()));
    let retiring = QueuedLanes::new();
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let first =
        registry.load_network_image(URL, retiring.services().with_http(first_http.handle()));
    retiring.run_until_idle();
    assert_eq!(
        first_http.requests_to(URL),
        1,
        "the lead's request is in flight"
    );
    let second =
        registry.load_network_image(URL, LoadServices::new().with_http(second_http.handle()));
    retiring.shut_down();
    assert_eq!(
        first_http.abandoned(),
        1,
        "the retired lanes dropped the request"
    );
    let image = block_on(second).expect("the waiter takes the load over");
    assert_eq!((image.width(), image.height()), (4, 2));
    assert_eq!(second_http.requests_to(URL), 1);
    drop(first);
}

/// Two waiters for one URL share one request. It stays in flight while
/// either waits, and dropping the last one abandons it.
#[test]
fn an_image_request_lives_exactly_as_long_as_someone_waits_for_it() {
    let http = ScriptedHttp::new();
    http.hold(URL);
    let lanes = ThreadLanes::new();
    let services = lanes.services().with_http(http.handle());
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let first = registry.load_network_image(URL, services.clone());
    let second = registry.load_network_image(URL, services);
    eventually(|| http.requests_to(URL) == 1, "the shared request goes out");

    drop(first);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(http.abandoned(), 0, "the request stays while another waits");
    assert_eq!(http.requests_to(URL), 1, "one request for both waiters");

    drop(second);
    eventually(
        || http.abandoned() == 1,
        "the last waiter's drop abandons it",
    );
}

/// Waits up to five seconds for `done`.
fn eventually(done: impl Fn() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

thread_local! {
    /// The client a [`reentering_waker`]'s clone calls back into.
    static REENTERED: std::cell::RefCell<Option<ScriptedHttp>> =
        const { std::cell::RefCell::new(None) };
}

/// A waker whose clone asks [`REENTERED`] for its requests, as an
/// executor's clone may run code that reaches the client. Safe `Wake`
/// cannot customise clone.
#[expect(
    unsafe_code,
    reason = "exercise an executor's clone through the public Future API"
)]
fn reentering_waker() -> std::task::Waker {
    use std::task::{RawWaker, RawWakerVTable, Waker};

    unsafe fn clone(data: *const ()) -> RawWaker {
        REENTERED.with(|http| {
            if let Some(http) = http.borrow().as_ref() {
                let _ = http.requests_to(URL);
            }
        });
        RawWaker::new(data, &VTABLE)
    }
    unsafe fn noop(_: *const ()) {}
    const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    // SAFETY: no vtable function dereferences `data`, and each is safe to call
    // from any thread any number of times.
    unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
}

/// A held request polled by a waker whose clone calls back into the client
/// stays pending instead of deadlocking on the client's own state.
#[test]
fn a_held_request_may_be_polled_by_a_waker_that_reenters_the_client() {
    let client = ScriptedHttp::new();
    client.hold(URL);
    let (done, polled) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        REENTERED.with(|reentered| *reentered.borrow_mut() = Some(client.clone()));
        let mut request = client
            .handle()
            .send(flui_platform::http::HttpRequest::get(URL));
        let waker = reentering_waker();
        let poll = request
            .as_mut()
            .poll(&mut std::task::Context::from_waker(&waker));
        let _ = done.send(poll.is_pending());
    });
    assert_eq!(
        polled.recv_timeout(Duration::from_secs(5)),
        Ok(true),
        "the held request registers its waker without holding the client's lock"
    );
}

/// Counts its wakes, and panics on each when told to.
struct CountingWaker {
    wakes: std::sync::atomic::AtomicUsize,
    panics: bool,
}

impl std::task::Wake for CountingWaker {
    fn wake(self: std::sync::Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &std::sync::Arc<Self>) {
        self.wakes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert!(!self.panics, "a waker that panics");
    }
}

fn counting_waker(panics: bool) -> std::sync::Arc<CountingWaker> {
    std::sync::Arc::new(CountingWaker {
        wakes: std::sync::atomic::AtomicUsize::new(0),
        panics,
    })
}

/// A held request keeps only the waker of its last poll, and lets it go
/// when the request is dropped unanswered.
#[test]
fn a_held_request_keeps_one_waker_and_lets_it_go_when_dropped() {
    let http = ScriptedHttp::new();
    http.hold(URL);
    let mut request = http
        .handle()
        .send(flui_platform::http::HttpRequest::get(URL));
    let first = counting_waker(false);
    let second = counting_waker(false);
    for waker in [&first, &second] {
        let waker = std::task::Waker::from(std::sync::Arc::clone(waker));
        assert!(
            request
                .as_mut()
                .poll(&mut std::task::Context::from_waker(&waker))
                .is_pending()
        );
    }
    assert_eq!(
        std::sync::Arc::strong_count(&first),
        1,
        "a repoll replaces the earlier waker"
    );
    assert_eq!(std::sync::Arc::strong_count(&second), 2);

    drop(request);
    assert_eq!(http.abandoned(), 1);
    assert_eq!(
        std::sync::Arc::strong_count(&second),
        1,
        "a dropped request lets its waker go"
    );
}

/// Releasing a URL wakes every held request even when their wakers panic,
/// then resumes the first panic.
#[test]
fn a_release_wakes_every_held_request_past_a_panicking_waker() {
    let http = ScriptedHttp::new();
    http.hold(URL);
    let waker = counting_waker(true);
    let requests: Vec<_> = (0..3)
        .map(|_| {
            let mut request = http
                .handle()
                .send(flui_platform::http::HttpRequest::get(URL));
            let polled = std::task::Waker::from(std::sync::Arc::clone(&waker));
            assert!(
                request
                    .as_mut()
                    .poll(&mut std::task::Context::from_waker(&polled))
                    .is_pending()
            );
            request
        })
        .collect();

    let released = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| http.release(URL)));
    assert!(released.is_err(), "the first panic is resumed");
    assert_eq!(
        waker.wakes.load(std::sync::atomic::Ordering::SeqCst),
        3,
        "every held request is woken"
    );
    drop(requests);
}
