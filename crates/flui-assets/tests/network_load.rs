//! Network image loads: fetched with the caller's HTTP client, on the lanes a
//! load is given or on none, and the typed errors of a failed fetch.

use std::time::{Duration, Instant};

use flui_assets::{AssetError, AssetRegistryBuilder, LoadServices};
use flui_platform::executor::{Lane, SpawnError};
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

/// The lead's lanes shut down under its held request; the request that
/// subscribed to that load on other lanes takes it over and fetches again.
fn handed_over() {
    use std::future::Future as _;
    use std::task::{Context, Poll, Waker};

    let http = ScriptedHttp::new();
    http.hold(URL);
    let retiring = QueuedLanes::new();
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let first = registry.load_network_image(URL, retiring.services().with_http(http.handle()));
    retiring.run_until_idle();
    assert_eq!(http.requests_to(URL), 1, "the lead's request is in flight");
    let mut second =
        Box::pin(registry.load_network_image(URL, LoadServices::new().with_http(http.handle())));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(second.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        http.requests_to(URL),
        1,
        "one client at one URL: the second load waits on the lead's request"
    );
    retiring.shut_down();
    assert_eq!(http.abandoned(), 1, "the retired lanes dropped the request");
    http.respond(URL, ScriptedResponse::ok(PNG.to_vec()));
    http.release(URL);
    let image = (0..64)
        .find_map(|_| match second.as_mut().poll(&mut cx) {
            Poll::Ready(answered) => Some(answered),
            Poll::Pending => None,
        })
        .expect("the waiter takes the load over")
        .expect("the takeover fetches the image");
    assert_eq!((image.width(), image.height()), (4, 2));
    assert_eq!(
        http.requests_to(URL),
        2,
        "the takeover sent its own request"
    );
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

/// A panic payload whose destructor panics too.
struct PanicsOnDrop;

impl Drop for PanicsOnDrop {
    fn drop(&mut self) {
        panic!("a panic payload that panics when dropped");
    }
}

/// Counts its wakes, and panics on each when told to, with a payload that
/// panics again when dropped.
fn counting_waker(panics: bool) -> std::sync::Arc<flui_foundation::testing::CountingWaker> {
    use flui_foundation::testing::CountingWaker;
    if panics {
        CountingWaker::calling(|| std::panic::panic_any(PanicsOnDrop))
    } else {
        CountingWaker::new()
    }
    .shared()
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
/// with payloads that panic again when dropped, then resumes the first panic.
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
    let first = released.expect_err("the first panic is resumed");
    assert!(first.is::<PanicsOnDrop>());
    // Its destructor panics: destroyed under containment, as `release` does
    // with the later ones.
    flui_foundation::panic::discard_payload(first);
    assert_eq!(waker.count(), 3, "every held request is woken");
    drop(requests);
}

/// A URL whose conversion asks the client for its requests, as a caller's
/// `Into<String>` may.
struct ReenteringUrl(ScriptedHttp);

impl From<ReenteringUrl> for String {
    fn from(url: ReenteringUrl) -> Self {
        let _ = url.0.requests_to(URL);
        URL.to_owned()
    }
}

/// Scripting or holding a URL whose conversion calls back into the client
/// does not deadlock on the client's state.
#[test]
fn a_url_conversion_may_reenter_the_client() {
    let client = ScriptedHttp::new();
    let (done, scripted) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        client.respond(
            ReenteringUrl(client.clone()),
            ScriptedResponse::ok(PNG.to_vec()),
        );
        client.hold(ReenteringUrl(client.clone()));
        let _ = done.send(());
    });
    assert_eq!(
        scripted.recv_timeout(Duration::from_secs(5)),
        Ok(()),
        "the URL is converted before the client's lock is taken"
    );
}

/// A 5×3 PNG, told apart from [`PNG`]'s 4×2.
const OTHER_PNG: &[u8] = include_bytes!("fixtures/tiny_5x3.png");

/// Two clients that answer one URL with different images, on one registry:
/// each load gets its own client's image, whether the other's is already
/// decoded or still in flight, and each client sees its own request.
///
/// Red-check: key network sources by URL alone (each load gets the image
/// of the client that fetched first).
#[test]
fn one_registry_keeps_two_clients_images_of_one_url_apart() {
    for in_flight in [false, true] {
        let first = ScriptedHttp::new();
        first.respond(URL, ScriptedResponse::ok(PNG.to_vec()));
        let second = ScriptedHttp::new();
        second.respond(URL, ScriptedResponse::ok(OTHER_PNG.to_vec()));
        let lanes = QueuedLanes::new();
        let registry = AssetRegistryBuilder::new().with_default_capacity().build();
        let first_load =
            registry.load_network_image(URL, lanes.services().with_http(first.handle()));
        if !in_flight {
            lanes.run_until_idle();
        }
        let second_load =
            registry.load_network_image(URL, lanes.services().with_http(second.handle()));
        lanes.run_until_idle();
        let first_image = block_on(first_load).expect("the first client's image loads");
        let second_image = block_on(second_load).expect("the second client's image loads");
        assert_eq!(
            (first_image.width(), first_image.height()),
            (4, 2),
            "in flight: {in_flight}"
        );
        assert_eq!(
            (second_image.width(), second_image.height()),
            (5, 3),
            "in flight: {in_flight}"
        );
        assert_eq!(first.requests_to(URL), 1, "in flight: {in_flight}");
        assert_eq!(second.requests_to(URL), 1, "in flight: {in_flight}");
        let cached = registry
            .cached_image(&flui_assets::ImageSource::network(URL, &second.handle()))
            .expect("the second client's image is kept");
        assert_eq!((cached.width(), cached.height()), (5, 3));
    }
}

/// A writer that asks the client it is formatting how many requests it sent.
struct ReenteringWriter {
    http: ScriptedHttp,
    asked: usize,
}

impl std::fmt::Write for ReenteringWriter {
    fn write_str(&mut self, _: &str) -> std::fmt::Result {
        let _sent: usize = self.http.requests_to(URL);
        self.asked += 1;
        Ok(())
    }
}

#[test]
fn a_debug_writer_may_reenter_the_client() {
    let http = ScriptedHttp::new();
    let (sender, written) = std::sync::mpsc::channel::<usize>();
    // On a thread of its own: formatting under the client's lock would wait
    // on itself forever, and the test fails instead.
    std::thread::spawn(move || {
        let mut writer = ReenteringWriter {
            http: http.clone(),
            asked: 0,
        };
        std::fmt::Write::write_fmt(&mut writer, format_args!("{http:?}"))
            .expect("the writer accepts everything");
        sender.send(writer.asked).expect("BUG: the test waits");
    });
    let asked = written
        .recv_timeout(Duration::from_secs(10))
        .expect("formatting finishes instead of waiting on its own lock");
    assert!(asked > 0, "the writer was called");
}

/// Polls `load` once, without a waker to call.
fn poll_once<F: std::future::Future + Unpin>(load: &mut F) -> std::task::Poll<F::Output> {
    std::pin::Pin::new(load).poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
}

/// The URL of the `n`th image in the slot tests.
fn nth_url(n: usize) -> String {
    format!("https://example.test/{n}.png")
}

/// One decode thread grants four load slots.
const LOAD_SLOTS: usize = 4;

#[test]
fn a_load_waiting_for_its_load_slot_holds_no_place_on_the_io_lane() {
    crate::cases::run_cases(&[
        (
            "spawned once its slot is freed",
            spawned_once_its_slot_is_freed,
        ),
        (
            "lanes with no room hand the run to the lead",
            no_room_leads_here,
        ),
        (
            "a panicking waker leaves the rest told",
            a_panicking_waker_leaves_the_rest_told,
        ),
        (
            "a panicking waker keeps its outcome",
            a_panicking_waker_keeps_its_outcome,
        ),
        (
            "lanes shutting down refuse the lead only",
            shutting_down_refuses_the_lead,
        ),
        (
            "a long queue is refused in turn",
            shutting_down_refuses_a_long_queue_in_turn,
        ),
    ]);
}

/// A registry with one decode thread, and `count` held loads on `lanes`.
fn held_loads(
    http: &ScriptedHttp,
    lanes: &std::sync::Arc<QueuedLanes>,
    count: usize,
) -> (flui_assets::AssetRegistry, Vec<flui_assets::ImageLoad>) {
    let registry = AssetRegistryBuilder::new()
        .with_default_capacity()
        .with_decode_threads(std::num::NonZeroUsize::MIN)
        .build();
    let loads = (0..count)
        .map(|n| {
            http.hold(nth_url(n));
            http.respond(nth_url(n), ScriptedResponse::ok(PNG.to_vec()));
            registry.load_network_image(nth_url(n), lanes.services().with_http(http.handle()))
        })
        .collect::<Vec<flui_assets::ImageLoad>>();
    lanes.run_io_until_idle();
    (registry, loads)
}

/// Six cold loads take four slots: the two queued ones are not on the IO
/// lane until a slot is freed for them, each in its turn.
fn spawned_once_its_slot_is_freed() {
    let http = ScriptedHttp::new();
    let lanes = QueuedLanes::new();
    let (_registry, mut loads) = held_loads(&http, &lanes, LOAD_SLOTS + 2);
    assert_eq!(
        lanes.pending_io(),
        LOAD_SLOTS,
        "queued loads are not spawned"
    );
    assert_eq!(http.requests().len(), LOAD_SLOTS);

    http.release(&nth_url(0));
    lanes.run_until_idle();
    assert!(poll_once(&mut loads[0]).is_ready());
    assert_eq!(
        lanes.pending_io(),
        LOAD_SLOTS,
        "the freed slot spawned the next load"
    );
    assert_eq!(http.requests_to(&nth_url(LOAD_SLOTS)), 1);
    assert_eq!(http.requests_to(&nth_url(LOAD_SLOTS + 1)), 0);

    for n in 1..LOAD_SLOTS + 2 {
        http.release(&nth_url(n));
    }
    lanes.run_until_idle();
    for load in &mut loads[1..] {
        let std::task::Poll::Ready(image) = poll_once(load) else {
            panic!("every load finished");
        };
        image.expect("every load decodes");
    }
    assert_eq!(lanes.pending_io(), 0);
}

/// The lanes take no more work when a slot comes free: the run is parked,
/// and the subscriber polled first drives it where it is polled, even when
/// the lead that queued it is never polled again.
fn no_room_leads_here() {
    let http = ScriptedHttp::new();
    let lanes = QueuedLanes::new();
    let (registry, mut loads) = held_loads(&http, &lanes, LOAD_SLOTS + 1);
    http.respond(
        nth_url(LOAD_SLOTS),
        ScriptedResponse::Fail(HttpError::Timeout),
    );
    let mut joined = registry.load_network_image(
        nth_url(LOAD_SLOTS),
        LoadServices::new().with_http(http.handle()),
    );
    assert!(poll_once(&mut joined).is_pending());
    lanes.refuse_lane(Lane::Io, SpawnError::Saturated);
    http.release(&nth_url(0));
    http.release(&nth_url(LOAD_SLOTS));
    lanes.run_until_idle();
    assert!(poll_once(&mut loads[0]).is_ready());
    assert_eq!(http.requests_to(&nth_url(LOAD_SLOTS)), 0, "not yet driven");

    // The parked run keeps the slot freed for it: a load from elsewhere
    // waits for one.
    let spare = QueuedLanes::new();
    let extra = registry.load_network_image(
        nth_url(LOAD_SLOTS + 1),
        spare.services().with_http(http.handle()),
    );
    assert_eq!(spare.pending_io(), 0, "every slot is held");

    let std::task::Poll::Ready(answered) = poll_once(&mut joined) else {
        panic!("the joined subscriber claimed the run and drove it to its end");
    };
    assert!(
        matches!(
            answered,
            Err(AssetError::Http {
                source: HttpError::Timeout,
                ..
            })
        ),
        "the load fails where it is polled"
    );
    assert_eq!(
        spare.pending_io(),
        1,
        "the run's slot went to the next load"
    );
    let std::task::Poll::Ready(answered) = poll_once(&mut loads[LOAD_SLOTS]) else {
        panic!("the lead hears the outcome");
    };
    assert!(
        matches!(
            answered,
            Err(AssetError::Http {
                source: HttpError::Timeout,
                ..
            })
        ),
        "the lead, told of the run after it was claimed, hears its outcome"
    );
    assert_eq!(http.requests_to(&nth_url(LOAD_SLOTS)), 1, "one request");
    drop(extra);
}

/// Panics on its first wake only.
struct PanicsOnFirstWake(std::sync::atomic::AtomicBool);

impl std::task::Wake for PanicsOnFirstWake {
    fn wake(self: std::sync::Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &std::sync::Arc<Self>) {
        assert!(
            self.0.swap(true, std::sync::atomic::Ordering::SeqCst),
            "a subscriber's waker panicked"
        );
    }
}

/// The run is parked with no room on the lanes, and the waker of a
/// subscriber told before the last panics: the last is still told, claims
/// the run and drives it, and the panic is raised once every one is told.
fn a_panicking_waker_leaves_the_rest_told() {
    let http = ScriptedHttp::new();
    let lanes = QueuedLanes::new();
    let (registry, mut loads) = held_loads(&http, &lanes, LOAD_SLOTS + 1);
    http.respond(
        nth_url(LOAD_SLOTS),
        ScriptedResponse::Fail(HttpError::Timeout),
    );
    let mut panicking = registry.load_network_image(
        nth_url(LOAD_SLOTS),
        LoadServices::new().with_http(http.handle()),
    );
    let waker = std::task::Waker::from(std::sync::Arc::new(PanicsOnFirstWake(
        std::sync::atomic::AtomicBool::new(false),
    )));
    assert!(
        std::pin::Pin::new(&mut panicking)
            .poll(&mut std::task::Context::from_waker(&waker))
            .is_pending()
    );
    let mut last = registry.load_network_image(
        nth_url(LOAD_SLOTS),
        LoadServices::new().with_http(http.handle()),
    );
    assert!(poll_once(&mut last).is_pending());
    lanes.refuse_lane(Lane::Io, SpawnError::Saturated);
    http.release(&nth_url(0));
    http.release(&nth_url(LOAD_SLOTS));
    let parked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lanes.run_until_idle()));
    assert!(parked.is_err(), "the waker's panic is raised");

    let std::task::Poll::Ready(answered) = poll_once(&mut last) else {
        panic!("the last subscriber, told past the panic, drove the run to its end");
    };
    assert!(
        matches!(
            answered,
            Err(AssetError::Http {
                source: HttpError::Timeout,
                ..
            })
        ),
        "the load fails where it is polled"
    );
    assert_eq!(http.requests_to(&nth_url(LOAD_SLOTS)), 1, "one request");
    drop((loads.drain(..), panicking));
}

/// A load settles while a subscriber's waker panics on its first wake: that
/// subscriber still hears the outcome rather than taking the load over.
fn a_panicking_waker_keeps_its_outcome() {
    let http = ScriptedHttp::new();
    let lanes = QueuedLanes::new();
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    http.respond(URL, ScriptedResponse::Fail(HttpError::Timeout));
    let lead = registry.load_network_image(URL, lanes.services().with_http(http.handle()));
    let mut panicking = registry.load_network_image(URL, lanes.services().with_http(http.handle()));
    let waker = std::task::Waker::from(std::sync::Arc::new(PanicsOnFirstWake(
        std::sync::atomic::AtomicBool::new(false),
    )));
    assert!(
        std::pin::Pin::new(&mut panicking)
            .poll(&mut std::task::Context::from_waker(&waker))
            .is_pending()
    );
    let settled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lanes.run_until_idle()));
    assert!(settled.is_err(), "the waker's panic is raised");

    let std::task::Poll::Ready(answered) = poll_once(&mut panicking) else {
        panic!("the subscriber whose waker panicked hears the outcome");
    };
    assert!(
        matches!(
            answered,
            Err(AssetError::Http {
                source: HttpError::Timeout,
                ..
            })
        ),
        "the outcome the run settled with"
    );
    assert_eq!(http.requests_to(URL), 1, "one request");
    drop(lead);
}

/// The lead's lanes shut down while it waits for a slot: the lead answers
/// `LanesRefused`, and a subscriber on other lanes takes the load over.
fn shutting_down_refuses_the_lead() {
    let http = ScriptedHttp::new();
    let busy = QueuedLanes::new();
    let (registry, mut loads) = held_loads(&http, &busy, LOAD_SLOTS);
    let retiring = QueuedLanes::new();
    let other = QueuedLanes::new();
    http.respond(URL, ScriptedResponse::ok(PNG.to_vec()));
    let mut lead = registry.load_network_image(URL, retiring.services().with_http(http.handle()));
    let mut joined = registry.load_network_image(URL, other.services().with_http(http.handle()));
    assert!(poll_once(&mut lead).is_pending());
    assert!(poll_once(&mut joined).is_pending());
    retiring.shut_down();

    http.release(&nth_url(0));
    busy.run_until_idle();
    assert!(poll_once(&mut loads[0]).is_ready());
    assert!(
        matches!(
            poll_once(&mut lead),
            std::task::Poll::Ready(Err(AssetError::LanesRefused {
                reason: SpawnError::ShuttingDown,
                ..
            }))
        ),
        "the lead's lanes refused it"
    );
    assert!(poll_once(&mut joined).is_pending());
    other.run_until_idle();
    let std::task::Poll::Ready(image) = poll_once(&mut joined) else {
        panic!("the joined subscriber took the load over");
    };
    image.expect("the takeover decodes");
    assert_eq!(http.requests_to(URL), 1);
}

/// Loads queued for slots on lanes that shut down are refused one after
/// another as the slot passes along the queue, not each from inside the
/// last one's refusal: on a thread with a small stack, a long queue drains
/// without overflowing it.
fn shutting_down_refuses_a_long_queue_in_turn() {
    const QUEUED: usize = 2_000;
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let http = ScriptedHttp::new();
            let busy = QueuedLanes::new();
            let (registry, mut loads) = held_loads(&http, &busy, LOAD_SLOTS);
            let retiring = QueuedLanes::new();
            let mut queued = (0..QUEUED)
                .map(|n| {
                    registry.load_network_image(
                        nth_url(LOAD_SLOTS + n),
                        retiring.services().with_http(http.handle()),
                    )
                })
                .collect::<Vec<flui_assets::ImageLoad>>();
            retiring.shut_down();
            http.release(&nth_url(0));
            busy.run_until_idle();
            assert!(poll_once(&mut loads[0]).is_ready());
            for load in &mut queued {
                assert!(
                    matches!(
                        poll_once(load),
                        std::task::Poll::Ready(Err(AssetError::LanesRefused { .. }))
                    ),
                    "every queued load is refused"
                );
            }
        })
        .expect("BUG: the test spawns one thread")
        .join()
        .expect("the queue drains on a small stack");
}
