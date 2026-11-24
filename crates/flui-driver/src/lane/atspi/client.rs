//! The AT-SPI client both Linux lanes read and act through: see the [parent module](super).
//!
//! The calls are zbus's, asynchronous, each waited for on the lane's thread with `block_on` and
//! bounded by the connection's method timeout; proxies cache no property, so every read is the
//! app's answer at that moment.

use std::time::{Duration, Instant};

use atspi::proxy::accessible::AccessibleProxy;
use atspi::proxy::bus::{BusProxy, StatusProxy};
use atspi::{CoordType, Interface, ObjectRefOwned, State, StateSet, zbus};
use flui_protocol::error::{ErrorCode, Retry};
use flui_protocol::session::Placement;
use flui_protocol::tree::{ElementId, Node, OUTLINE_TEXT_LIMIT, Rect, Tree};
use flui_protocol::wire::Checked;
use futures_lite::future::block_on;

use super::CALL_TIMEOUT;
use super::roles::{self, AtspiRole};
use crate::error::{Error, Result};
use crate::lane::walk::{self, Properties, Provider, ProviderError};

/// The registry's root, whose children are the applications on the bus.
const REGISTRY: &str = "org.a11y.atspi.Registry";
const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";

/// How often the client looks again for an app that has not registered its tree yet.
const LOOK_AGAIN: Duration = Duration::from_millis(50);

/// The window of the app an AT-SPI read is for: the app's process, and the window's size in
/// device pixels, which tells its window apart from the app's others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowTarget {
    /// The app's process.
    pub(crate) pid: u32,
    /// The window's width and height in device pixels.
    pub(crate) size: (u32, u32),
}

impl WindowTarget {
    /// The window of process `pid` that `placement` describes: its logical size times its scale,
    /// rounded to whole device pixels; a size that is not a finite number inside `u32`'s range
    /// reads as 0, which no window has.
    pub(crate) fn of(pid: u32, placement: &Placement) -> Self {
        let device = |logical: f64| {
            let device = (logical * placement.scale).round();
            if device.is_finite() && device >= 0.0 && device <= f64::from(u32::MAX) {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a whole, finite, non-negative number checked inside u32's range"
                )]
                let device = device as u32;
                device
            } else {
                0
            }
        };
        Self {
            pid,
            size: (device(placement.size.width), device(placement.size.height)),
        }
    }
}

/// A connection to the accessibility bus.
pub(crate) struct A11yBus {
    bus: zbus::Connection,
    lane: &'static str,
}

fn unavailable(lane: &'static str, message: String) -> Error {
    Error::Lane {
        lane: lane.to_owned(),
        unavailable: true,
        message,
    }
}

fn classify(error: &zbus::Error) -> ProviderError {
    match error {
        zbus::Error::InputOutput(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            ProviderError::Timeout
        }
        zbus::Error::MethodError(name, _, _) => match name.as_str() {
            "org.freedesktop.DBus.Error.NoReply" | "org.freedesktop.DBus.Error.Timeout" => {
                ProviderError::Timeout
            }
            "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.UnknownObject"
            | "org.freedesktop.DBus.Error.NameHasNoOwner" => ProviderError::Gone,
            _ => ProviderError::Failed(error.to_string()),
        },
        zbus::Error::FDO(fdo) => match fdo.as_ref() {
            zbus::fdo::Error::NoReply(_) | zbus::fdo::Error::Timeout(_) => ProviderError::Timeout,
            zbus::fdo::Error::ServiceUnknown(_)
            | zbus::fdo::Error::UnknownObject(_)
            | zbus::fdo::Error::NameHasNoOwner(_) => ProviderError::Gone,
            _ => ProviderError::Failed(error.to_string()),
        },
        _ => ProviderError::Failed(error.to_string()),
    }
}

impl A11yBus {
    /// The accessibility bus of the session bus at `session` (the process's own when `None`), for
    /// the lane named `lane`.
    ///
    /// # Errors
    ///
    /// [`Error::Lane`] with `unavailable` when the session has no accessibility bus, or AT-SPI is
    /// off in it.
    pub(crate) fn connect(session: Option<&str>, lane: &'static str) -> Result<Self> {
        block_on(async {
            let builder = match session {
                Some(address) => zbus::connection::Builder::address(address),
                None => zbus::connection::Builder::session(),
            }
            .map_err(|error| unavailable(lane, format!("no session bus: {error}")))?;
            let session = builder
                .method_timeout(CALL_TIMEOUT)
                .build()
                .await
                .map_err(|error| unavailable(lane, format!("no session bus: {error}")))?;
            let status = StatusProxy::new(&session).await.map_err(|error| {
                unavailable(
                    lane,
                    format!("the session has no accessibility bus: {error}"),
                )
            })?;
            let enabled = status.is_enabled().await.map_err(|error| {
                unavailable(
                    lane,
                    format!("the session has no accessibility bus: {error}"),
                )
            })?;
            if !enabled {
                return Err(unavailable(
                    lane,
                    "AT-SPI is off in this session (org.a11y.Status.IsEnabled): an app publishes \
                     no tree until a screen reader turns it on, and the lane does not change the \
                     setting"
                        .to_owned(),
                ));
            }
            let address = BusProxy::new(&session)
                .await
                .map_err(|error| unavailable(lane, format!("no accessibility bus: {error}")))?
                .get_address()
                .await
                .map_err(|error| unavailable(lane, format!("no accessibility bus: {error}")))?;
            let bus = zbus::connection::Builder::address(address.as_str())
                .map_err(|error| unavailable(lane, format!("no accessibility bus: {error}")))?
                .method_timeout(CALL_TIMEOUT)
                .build()
                .await
                .map_err(|error| unavailable(lane, format!("no accessibility bus: {error}")))?;
            Ok(Self { bus, lane })
        })
    }

    async fn proxy(&self, object: &ObjectRefOwned) -> zbus::Result<AccessibleProxy<'static>> {
        let name = object
            .name_as_str()
            .ok_or_else(|| zbus::Error::Failure("an object with no bus name".to_owned()))?
            .to_owned();
        AccessibleProxy::builder(&self.bus)
            .destination(name)?
            .path(object.path_as_str().to_owned())?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
    }

    /// The window `target` names, once its app has registered it: see the
    /// [parent module](super).
    async fn window(&self, target: WindowTarget) -> Result<ObjectRefOwned> {
        let root = AccessibleProxy::builder(&self.bus)
            .destination(REGISTRY)
            .and_then(|builder| builder.path(ROOT_PATH))
            .map_err(|error| unavailable(self.lane, format!("no AT-SPI registry: {error}")))?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|error| unavailable(self.lane, format!("no AT-SPI registry: {error}")))?;
        let names = zbus::fdo::DBusProxy::new(&self.bus)
            .await
            .map_err(|error| unavailable(self.lane, format!("no bus daemon: {error}")))?;
        let deadline = Instant::now() + CALL_TIMEOUT;
        loop {
            let applications = root
                .get_children()
                .await
                .map_err(|error| provider_error(classify(&error), "the bus's applications"))?;
            let mut windows = Vec::new();
            for application in applications {
                let Some(name) = application.name_as_str() else {
                    continue;
                };
                let Ok(bus_name) = zbus::names::BusName::try_from(name.to_owned()) else {
                    continue;
                };
                let owner = names.get_connection_unix_process_id(bus_name).await;
                if owner.ok() != Some(target.pid) {
                    continue;
                }
                // The app's own answers: one that times out fails the read with a typed timeout,
                // and an app that went away is left out.
                let windows_of = async {
                    let application = self.proxy(&application).await.map_err(|e| classify(&e))?;
                    let mut found = Vec::new();
                    for window in application.get_children().await.map_err(|e| classify(&e))? {
                        if self.has_size(&window, target.size).await? {
                            found.push(window);
                        }
                    }
                    Ok::<Vec<ObjectRefOwned>, ProviderError>(found)
                };
                match windows_of.await {
                    Ok(found) => windows.extend(found),
                    Err(ProviderError::Gone) => {}
                    Err(error) => return Err(provider_error(error, "the app's windows")),
                }
            }
            match windows.len() {
                1 => return Ok(windows.remove(0)),
                0 if Instant::now() < deadline => {
                    async_io::Timer::after(LOOK_AGAIN).await;
                }
                0 => {
                    return Err(Error::not_found(format!(
                        "process {} shows no window of {} x {} device pixels on the \
                         accessibility bus",
                        target.pid, target.size.0, target.size.1
                    )));
                }
                several => {
                    return Err(Error::Lane {
                        lane: self.lane.to_owned(),
                        unavailable: false,
                        message: format!(
                            "process {} shows {several} windows of {} x {} device pixels on the \
                             accessibility bus; the lane cannot tell which is the app's",
                            target.pid, target.size.0, target.size.1
                        ),
                    });
                }
            }
        }
    }

    /// Whether `window` is `size` device pixels, within the pixel a server rounds to; false for a
    /// window that offers no Component interface, which places nothing on the screen.
    ///
    /// # Errors
    ///
    /// The app's failure to answer, classified: a timeout, a window gone, any other.
    async fn has_size(
        &self,
        window: &ObjectRefOwned,
        size: (u32, u32),
    ) -> std::result::Result<bool, ProviderError> {
        let proxy = self.proxy(window).await.map_err(|e| classify(&e))?;
        if !proxy
            .get_interfaces()
            .await
            .map_err(|e| classify(&e))?
            .contains(Interface::Component)
        {
            return Ok(false);
        }
        let proxies = atspi::proxy::proxy_ext::ProxyExt::proxies(&proxy)
            .await
            .map_err(|e| ProviderError::Failed(e.to_string()))?;
        let component = proxies
            .component()
            .await
            .map_err(|e| ProviderError::Failed(e.to_string()))?;
        let (_, _, width, height) = component
            .get_extents(CoordType::Screen)
            .await
            .map_err(|e| classify(&e))?;
        let near = |found: i32, wanted: u32| (i64::from(found) - i64::from(wanted)).abs() <= 1;
        Ok(near(width, size.0) && near(height, size.1))
    }

    /// The tree of the window `target` names, projected: see the [parent module](super).
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] when the app shows no such window; [`Error::Timeout`] when the app did
    /// not answer in time; [`Error::Platform`] when the tree could not be read.
    pub(crate) fn read(&self, target: WindowTarget) -> Result<Tree> {
        let window = block_on(self.window(target))?;
        let mut provider = Reader { bus: self };
        walk::read(&mut provider, &Element::root(window), CALL_TIMEOUT)
    }

    /// Activate the element of the window `target` names whose `AccessibleId` is `identifier`,
    /// through its `click` action (its first, where it names none so), as a screen reader's
    /// activate command does.
    ///
    /// # Errors
    ///
    /// As [`walk::find`], [`walk::pattern`] for its Action interface and [`walk::invoked`].
    pub(crate) fn invoke(&self, target: WindowTarget, identifier: &str) -> Result<()> {
        let window = block_on(self.window(target))?;
        let mut provider = Reader { bus: self };
        let element = walk::find(
            &mut provider,
            Element::root(window),
            identifier,
            CALL_TIMEOUT,
        )?;
        let action = block_on(async {
            let proxy = self
                .proxy(&element.object)
                .await
                .map_err(|e| classify(&e))?;
            let interfaces = proxy.get_interfaces().await.map_err(|e| classify(&e))?;
            if !interfaces.contains(Interface::Action) {
                return Ok(None);
            }
            let proxies = atspi::proxy::proxy_ext::ProxyExt::proxies(&proxy)
                .await
                .map_err(|e| ProviderError::Failed(e.to_string()))?;
            let action = proxies
                .action()
                .await
                .map_err(|e| ProviderError::Failed(e.to_string()))?;
            let names = action.get_actions().await.map_err(|e| classify(&e))?;
            if names.is_empty() {
                return Ok(None);
            }
            let index = names
                .iter()
                .position(|named| named.name == "click")
                .unwrap_or(0);
            Ok(Some((action, i32::try_from(index).unwrap_or(0))))
        });
        let (action, index) = walk::pattern(action, identifier, "action", CALL_TIMEOUT)?;
        let answer = block_on(action.do_action(index)).map_err(|e| classify(&e));
        let answer = match answer {
            Ok(true) => Ok(()),
            Ok(false) => Err(ProviderError::Failed(
                "the app refused the action".to_owned(),
            )),
            Err(error) => Err(error),
        };
        walk::invoked(answer, identifier, CALL_TIMEOUT)
    }
}

fn provider_error(error: ProviderError, what: &str) -> Error {
    match error {
        ProviderError::Timeout => Error::Timeout {
            awaited: crate::error::Awaited::Host,
            effect: None,
            message: format!(
                "the accessibility bus did not answer for {what} within {} ms",
                CALL_TIMEOUT.as_millis()
            ),
            retry: ErrorCode::Timeout.default_retry(),
        },
        ProviderError::Gone | ProviderError::Failed(_) => Error::Platform {
            effect: None,
            message: format!("the accessibility bus could not list {what}: {error:?}"),
            retry: Retry::Never,
        },
    }
}

/// One element of the walk: the object, and its parent's children with its place among them, so
/// a step to the next sibling asks the bus nothing.
#[derive(Clone)]
pub(crate) struct Element {
    object: ObjectRefOwned,
    siblings: std::rc::Rc<Vec<ObjectRefOwned>>,
    index: usize,
}

impl Element {
    fn root(object: ObjectRefOwned) -> Self {
        Self {
            siblings: std::rc::Rc::new(Vec::new()),
            object,
            index: 0,
        }
    }
}

struct Reader<'a> {
    bus: &'a A11yBus,
}

impl Provider for Reader<'_> {
    type Element = Element;

    fn first_child(
        &mut self,
        element: &Element,
    ) -> std::result::Result<Option<Element>, ProviderError> {
        let children = block_on(async {
            let proxy = self
                .bus
                .proxy(&element.object)
                .await
                .map_err(|e| classify(&e))?;
            proxy.get_children().await.map_err(|e| classify(&e))
        })?;
        if children.is_empty() {
            return Ok(None);
        }
        let siblings = std::rc::Rc::new(children);
        Ok(Some(Element {
            object: siblings[0].clone(),
            siblings,
            index: 0,
        }))
    }

    fn next_sibling(
        &mut self,
        element: &Element,
    ) -> std::result::Result<Option<Element>, ProviderError> {
        let index = element.index + 1;
        Ok(element.siblings.get(index).map(|object| Element {
            object: object.clone(),
            siblings: std::rc::Rc::clone(&element.siblings),
            index,
        }))
    }

    fn node(
        &mut self,
        element: &Element,
        id: ElementId,
    ) -> std::result::Result<Node, ProviderError> {
        describe(self.bus, &element.object, id)
    }
}

/// Make the call `call` through `reads`, unless an earlier one timed out or found the object
/// gone.
fn keep<T>(
    reads: &mut Properties,
    call: impl std::future::Future<Output = zbus::Result<T>>,
) -> Option<T> {
    reads.read(|| block_on(call).map_err(|error| classify(&error)))
}

/// The first [`OUTLINE_TEXT_LIMIT`] characters of `text`; `None` for an empty one.
fn cut(text: &str) -> Option<String> {
    let text: String = text.chars().take(OUTLINE_TEXT_LIMIT).collect();
    (!text.is_empty()).then_some(text)
}

/// One object as a node, its reads made through [`Properties`]: after the first that times out or
/// finds the object gone no further call is made.
fn describe(
    bus: &A11yBus,
    object: &ObjectRefOwned,
    id: ElementId,
) -> std::result::Result<Node, ProviderError> {
    let proxy = block_on(bus.proxy(object)).map_err(|e| classify(&e))?;
    let mut reads = Properties::new();
    let role = keep(&mut reads, proxy.get_role())
        .map_or(AtspiRole::UNKNOWN, |role| AtspiRole(role as u32));
    let mut node = Node::new(id, roles::wire_role(role), role.native_name());
    node.name = keep(&mut reads, proxy.name()).as_deref().and_then(cut);
    node.description = keep(&mut reads, proxy.description())
        .as_deref()
        .and_then(cut);
    node.automation_id = keep(&mut reads, proxy.accessible_id())
        .as_deref()
        .and_then(cut);
    let state = keep(&mut reads, proxy.get_state()).unwrap_or_else(StateSet::empty);
    let interfaces = keep(&mut reads, proxy.get_interfaces());
    let proxies = block_on(atspi::proxy::proxy_ext::ProxyExt::proxies(&proxy)).ok();
    if let (Some(interfaces), Some(proxies)) = (interfaces, proxies.as_ref()) {
        if interfaces.contains(Interface::EditableText)
            && interfaces.contains(Interface::Text)
            && let Ok(text) = block_on(proxies.text())
        {
            // A field has a value, the empty text of an empty field included.
            node.value = keep(&mut reads, text.get_text(0, -1))
                .map(|text| text.chars().take(OUTLINE_TEXT_LIMIT).collect::<String>());
        }
        if interfaces.contains(Interface::Component)
            && let Ok(component) = block_on(proxies.component())
            && let Some((x, y, width, height)) =
                keep(&mut reads, component.get_extents(CoordType::Screen))
            && let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height))
            && width > 0
            && height > 0
        {
            node.rect = Some(Rect {
                x,
                y,
                width,
                height,
            });
        }
    }
    node.checked = checked(role, state);
    if state.contains(State::Expandable) {
        node.expanded = Some(state.contains(State::Expanded));
    }
    if state.contains(State::Selectable) {
        node.selected = Some(state.contains(State::Selected));
    }
    // The adapter marks a node it does not disable enabled and sensitive, or read-only.
    node.disabled = !state.contains(State::Enabled) && !state.contains(State::ReadOnly);
    node.focused = state.contains(State::Focused);
    node.focusable = state.contains(State::Focusable);
    reads.finish(node)
}

/// The checked state of a node of `role` in `state`: a checkable node's, or a toggle button's,
/// which AT-SPI reports as pressed.
fn checked(role: AtspiRole, state: StateSet) -> Option<Checked> {
    let toggle = role == AtspiRole::TOGGLE_BUTTON;
    if !toggle && !state.contains(State::Checkable) {
        return None;
    }
    Some(if state.contains(State::Indeterminate) {
        Checked::Mixed
    } else if state.contains(State::Checked) || (toggle && state.contains(State::Pressed)) {
        Checked::True
    } else {
        Checked::False
    })
}
