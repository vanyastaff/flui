//! XIM's X transport: a connection of the client's own to the X server, the
//! handshake that finds the input method's server and its communication
//! window, and messages carried between the two windows in client messages
//! or, past twenty bytes, in a property named by one.
//!
//! The connection is read on the bus task through its descriptor's
//! readiness. A round trip (an atom, a property) waits on the server, which
//! answers it at once; the events read meanwhile stay queued for
//! [`Link::receive`].

use std::os::fd::{AsRawFd, RawFd};
use std::time::Duration;

use tokio::io::unix::AsyncFd;
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    self, AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, PropMode,
    WindowClass,
};
use x11rb::rust_connection::RustConnection;

use super::wire::MAX_MESSAGE;

/// How long the server has to answer the handshake.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// How many properties carry messages at once: each message past twenty
/// bytes takes the next, so one is reused after this many more were sent,
/// long after the peer read and deleted it.
const DATA_PROPERTIES: usize = 64;

/// The atoms the transport names.
#[derive(Clone, Copy, Debug)]
struct Atoms {
    servers: xproto::Atom,
    #[cfg(test)]
    locales: xproto::Atom,
    transport: xproto::Atom,
    xconnect: xproto::Atom,
    protocol: xproto::Atom,
    moredata: xproto::Atom,
}

impl Atoms {
    fn intern(conn: &RustConnection) -> Option<Self> {
        let atom = |name: &[u8]| -> Option<xproto::Atom> {
            Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom)
        };
        Some(Self {
            servers: atom(b"XIM_SERVERS")?,
            #[cfg(test)]
            locales: atom(b"LOCALES")?,
            transport: atom(b"TRANSPORT")?,
            xconnect: atom(b"_XIM_XCONNECT")?,
            protocol: atom(b"_XIM_PROTOCOL")?,
            moredata: atom(b"_XIM_MOREDATA")?,
        })
    }
}

/// The connection, owned by the watch on its descriptor.
struct Watched(RustConnection);

impl AsRawFd for Watched {
    fn as_raw_fd(&self) -> RawFd {
        self.0.stream().as_raw_fd()
    }
}

/// A connection to the X server and the readiness of its descriptor.
pub(crate) struct Display {
    watched: AsyncFd<Watched>,
    root: xproto::Window,
    atoms: Atoms,
}

impl Display {
    /// Connect to `display`, or to `DISPLAY`'s.
    pub(crate) fn connect(display: Option<&str>) -> Option<Self> {
        let (conn, screen) = x11rb::connect(display)
            .inspect_err(|error| tracing::debug!(%error, "no X connection for XIM"))
            .ok()?;
        let root = conn.setup().roots.get(screen)?.root;
        let atoms = Atoms::intern(&conn)?;
        #[expect(
            unsafe_code,
            reason = "watches the X connection's socket; the block names its contract"
        )]
        // SAFETY: `Watched` owns the connection, which owns the socket and
        // never replaces it, so the descriptor stays open and the same for
        // as long as the watch holds it.
        let watched = unsafe { AsyncFd::register(Watched(conn)) }
            .inspect_err(|error| tracing::debug!(%error, "the XIM connection is not watched"))
            .ok()?;
        Some(Self {
            watched,
            root,
            atoms,
        })
    }

    fn conn(&self) -> &RustConnection {
        &self.watched.get_ref().0
    }

    /// A window of the transport's own, unmapped and taking no input.
    fn window(&self) -> Option<xproto::Window> {
        let window = self.conn().generate_id().ok()?;
        self.conn()
            .create_window(
                0,
                window,
                self.root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                0,
                &CreateWindowAux::new(),
            )
            .ok()?;
        Some(window)
    }

    /// The next event, waiting for one.
    pub(crate) async fn event(&self) -> Option<Event> {
        loop {
            match self.conn().poll_for_event() {
                Ok(Some(event)) => return Some(event),
                Ok(None) => {}
                Err(error) => {
                    tracing::debug!(%error, "the XIM connection broke");
                    return None;
                }
            }
            let mut ready = self.watched.readable().await.ok()?;
            ready.clear_ready();
        }
    }

    /// The next event before `deadline`.
    async fn event_by(&self, deadline: tokio::time::Instant) -> Option<Event> {
        tokio::time::timeout_at(deadline, self.event())
            .await
            .ok()
            .flatten()
    }

    fn flush(&self) -> bool {
        self.conn()
            .flush()
            .inspect_err(|error| tracing::debug!(%error, "the XIM connection broke"))
            .is_ok()
    }

    /// Up to [`MAX_MESSAGE`] bytes of `window`'s `property`, which is
    /// deleted; `None` when it is longer.
    fn take_property(
        &self,
        window: xproto::Window,
        property: xproto::Atom,
        length: u32,
    ) -> Option<Vec<u8>> {
        let length = usize::try_from(length).ok()?;
        if length > MAX_MESSAGE {
            return None;
        }
        let units = u32::try_from(length.div_ceil(4)).ok()?;
        let reply = self
            .conn()
            .get_property(true, window, property, AtomEnum::ANY, 0, units)
            .ok()?
            .reply()
            .ok()?;
        let mut value = reply.value;
        value.truncate(length);
        Some(value)
    }

    fn send_client_message(
        &self,
        to: xproto::Window,
        kind: xproto::Atom,
        data: xproto::ClientMessageData,
        format: u8,
    ) -> bool {
        let event = ClientMessageEvent {
            response_type: xproto::CLIENT_MESSAGE_EVENT,
            format,
            sequence: 0,
            window: to,
            type_: kind,
            data,
        };
        self.conn()
            .send_event(false, to, EventMask::NO_EVENT, event)
            .is_ok()
    }
}

/// One end of an XIM connection: the window this end reads messages on and
/// the peer's it writes them to.
pub(crate) struct Link {
    display: Display,
    window: xproto::Window,
    peer: xproto::Window,
    /// The properties messages past twenty bytes go in, interned as first
    /// used, and the next one to use.
    data: Vec<xproto::Atom>,
    next_data: usize,
    /// The twenty-byte pieces of a message sent in several client messages.
    pieces: Vec<u8>,
}

/// What [`Link::receive`] read.
pub(crate) enum Received {
    /// A whole message's bytes.
    Message(Vec<u8>),
    /// An event the transport does not carry messages in.
    Other,
    /// The peer went or broke the transport, or the connection closed.
    Gone,
}

impl Link {
    /// Connect as a client to the input method server named `name`
    /// (`XMODIFIERS=@im=name`).
    pub(crate) async fn client(display: Display, name: &str) -> Option<Self> {
        let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
        let atoms = display.atoms;
        let window = display.window()?;
        let server = find_server(&display, name)?;
        let owner = display
            .conn()
            .get_selection_owner(server)
            .ok()?
            .reply()
            .ok()?
            .owner;
        if owner == x11rb::NONE {
            return None;
        }
        // The server says how it is reached: the X transport is the one
        // servers offer.
        display
            .conn()
            .convert_selection(
                window,
                server,
                atoms.transport,
                atoms.transport,
                x11rb::CURRENT_TIME,
            )
            .ok()?;
        display.flush();
        let transport = loop {
            match display.event_by(deadline).await? {
                Event::SelectionNotify(notify) if notify.requestor == window => {
                    if notify.property == x11rb::NONE {
                        return None;
                    }
                    break display.take_property(window, notify.property, 1024)?;
                }
                _ => {}
            }
        };
        if !transport
            .split(|&b| b == b',')
            .any(|entry| entry.starts_with(b"@transport=X/") || entry == b"X/")
        {
            tracing::debug!("the XIM server offers no X transport");
            return None;
        }
        // The server's going ends the link.
        display
            .conn()
            .change_window_attributes(
                owner,
                &xproto::ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY),
            )
            .ok()?;
        if !display.send_client_message(owner, atoms.xconnect, [window, 0, 0, 0, 0].into(), 32) {
            return None;
        }
        display.flush();
        let peer = loop {
            match display.event_by(deadline).await? {
                Event::ClientMessage(message)
                    if message.window == window && message.type_ == atoms.xconnect =>
                {
                    let [peer, ..] = message.data.as_data32();
                    break peer;
                }
                Event::DestroyNotify(destroyed) if destroyed.window == owner => return None,
                _ => {}
            }
        };
        if peer == x11rb::NONE {
            return None;
        }
        // The communication window going ends the link too.
        let _ = display.conn().change_window_attributes(
            peer,
            &xproto::ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY),
        );
        display.flush();
        Some(Self {
            display,
            window,
            peer,
            data: Vec::new(),
            next_data: 0,
            pieces: Vec::new(),
        })
    }

    /// The connection, for the requests the session makes of the X server.
    pub(crate) fn conn(&self) -> &RustConnection {
        self.display.conn()
    }

    /// The root window of the connection's screen.
    pub(crate) fn root(&self) -> xproto::Window {
        self.display.root
    }

    /// Send `message`, encoded: in one client message up to twenty bytes,
    /// else in a property the client message names.
    pub(crate) fn send(&mut self, message: &[u8]) -> bool {
        let atoms = self.display.atoms;
        let sent = if message.len() <= 20 {
            let mut data = [0u8; 20];
            data[..message.len()].copy_from_slice(message);
            self.display
                .send_client_message(self.peer, atoms.protocol, data.into(), 8)
        } else {
            let Some(property) = self.data_property() else {
                return false;
            };
            let Ok(length) = u32::try_from(message.len()) else {
                return false;
            };
            self.display
                .conn()
                .change_property(
                    PropMode::APPEND,
                    self.peer,
                    property,
                    AtomEnum::STRING,
                    8,
                    length,
                    message,
                )
                .is_ok()
                && self.display.send_client_message(
                    self.peer,
                    atoms.protocol,
                    [length, property, 0, 0, 0].into(),
                    32,
                )
        };
        sent && self.display.flush()
    }

    /// The property the next long message goes in.
    fn data_property(&mut self) -> Option<xproto::Atom> {
        let index = self.next_data;
        self.next_data = (self.next_data + 1) % DATA_PROPERTIES;
        if let Some(atom) = self.data.get(index) {
            return Some(*atom);
        }
        let name = format!("_FLUI_XIM_DATA_{index}");
        let atom = self
            .display
            .conn()
            .intern_atom(false, name.as_bytes())
            .ok()?
            .reply()
            .ok()?
            .atom;
        self.data.push(atom);
        Some(atom)
    }

    /// The next message from the peer, or the next event that carries none.
    pub(crate) async fn receive(&mut self) -> Received {
        let atoms = self.display.atoms;
        loop {
            let Some(event) = self.display.event().await else {
                return Received::Gone;
            };
            let message = match event {
                Event::ClientMessage(message) if message.window == self.window => message,
                Event::DestroyNotify(destroyed) if destroyed.window == self.peer => {
                    return Received::Gone;
                }
                _ => return Received::Other,
            };
            if message.type_ == atoms.moredata && message.format == 8 {
                if self.pieces.len() + 20 > MAX_MESSAGE {
                    tracing::debug!("an XIM message past the longest read: the link ends");
                    return Received::Gone;
                }
                self.pieces.extend_from_slice(&message.data.as_data8());
                continue;
            }
            if message.type_ != atoms.protocol {
                continue;
            }
            match message.format {
                8 => {
                    let mut bytes = std::mem::take(&mut self.pieces);
                    bytes.extend_from_slice(&message.data.as_data8());
                    return Received::Message(bytes);
                }
                32 => {
                    let [length, property, ..] = message.data.as_data32();
                    let Some(bytes) = self.display.take_property(self.window, property, length)
                    else {
                        tracing::debug!("an XIM message past the longest read: the link ends");
                        return Received::Gone;
                    };
                    return Received::Message(bytes);
                }
                _ => {}
            }
        }
    }
}

/// The selection naming the server `name` among those `XIM_SERVERS` lists.
fn find_server(display: &Display, name: &str) -> Option<xproto::Atom> {
    let listed = display
        .conn()
        .get_property(
            false,
            display.root,
            display.atoms.servers,
            AtomEnum::ATOM,
            0,
            1024,
        )
        .ok()?
        .reply()
        .ok()?;
    let wanted = format!("@server={name}");
    listed.value32()?.find(|server| {
        display
            .conn()
            .get_atom_name(*server)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| reply.name == wanted.as_bytes())
    })
}

/// The server's end, for tests: a stand-in input method server.
#[cfg(test)]
pub(crate) mod server {
    use super::*;
    use x11rb::wrapper::ConnectionExt as _;

    /// A server registered as `name`, waiting for a client.
    pub(crate) struct Listener {
        display: Display,
        window: xproto::Window,
    }

    impl Listener {
        pub(crate) fn start(display: Display, name: &str) -> Option<Self> {
            let window = display.window()?;
            let server = display
                .conn()
                .intern_atom(false, format!("@server={name}").as_bytes())
                .ok()?
                .reply()
                .ok()?
                .atom;
            display
                .conn()
                .set_selection_owner(window, server, x11rb::CURRENT_TIME)
                .ok()?;
            display
                .conn()
                .change_property32(
                    PropMode::PREPEND,
                    display.root,
                    display.atoms.servers,
                    AtomEnum::ATOM,
                    &[server],
                )
                .ok()?;
            display.flush();
            Some(Self { display, window })
        }

        /// Answer the handshake of the next client: its link.
        pub(crate) async fn accept(self) -> Option<Link> {
            let atoms = self.display.atoms;
            loop {
                match self.display.event().await? {
                    Event::SelectionRequest(request) if request.owner == self.window => {
                        let value: &[u8] = if request.target == atoms.transport {
                            b"@transport=X/"
                        } else if request.target == atoms.locales {
                            b"@locale=C,en_US"
                        } else {
                            b""
                        };
                        self.display
                            .conn()
                            .change_property8(
                                PropMode::REPLACE,
                                request.requestor,
                                request.property,
                                request.target,
                                value,
                            )
                            .ok()?;
                        let notify = xproto::SelectionNotifyEvent {
                            response_type: xproto::SELECTION_NOTIFY_EVENT,
                            sequence: 0,
                            time: request.time,
                            requestor: request.requestor,
                            selection: request.selection,
                            target: request.target,
                            property: request.property,
                        };
                        self.display
                            .conn()
                            .send_event(false, request.requestor, EventMask::NO_EVENT, notify)
                            .ok()?;
                        self.display.flush();
                    }
                    Event::ClientMessage(message)
                        if message.window == self.window && message.type_ == atoms.xconnect =>
                    {
                        let [client, ..] = message.data.as_data32();
                        let window = self.display.window()?;
                        self.display.send_client_message(
                            client,
                            atoms.xconnect,
                            [window, 0, 0, 20, 0].into(),
                            32,
                        );
                        self.display.flush();
                        return Some(Link {
                            display: self.display,
                            window,
                            peer: client,
                            data: Vec::new(),
                            next_data: 0,
                            pieces: Vec::new(),
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    impl Link {
        /// Send raw client messages, for a test of a peer that misbehaves.
        pub(crate) fn send_raw(&self, kind: &[u8], data: [u8; 20]) -> bool {
            let Some(atom) = self
                .display
                .conn()
                .intern_atom(false, kind)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
            else {
                return false;
            };
            self.display
                .send_client_message(self.peer, atom.atom, data.into(), 8)
                && self.display.flush()
        }

        /// Destroy this end's window, as a server that goes.
        pub(crate) fn close(&self) {
            let _ = self.display.conn().destroy_window(self.window);
            self.display.flush();
        }
    }
}
