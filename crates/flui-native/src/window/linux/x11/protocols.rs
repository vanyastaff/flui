//! What an X11 window answers its window manager with beyond its close
//! (ICCCM `WM_PROTOCOLS`, EWMH): the name of the machine its process runs on,
//! and when it sets the counters a synchronised resize waits on.
//!
//! # The machine
//!
//! A window manager that finds a window no longer answering its pings
//! (`_NET_WM_PING`) offers to end the process `_NET_WM_PID` names, which it
//! may do only on the machine `WM_CLIENT_MACHINE` names: EWMH requires the
//! machine wherever the process is written. [`ClientMachine`] is that name as
//! the property carries it; [`ClientProcess`] keeps the id only beside a
//! machine and only where the window manager's id for the process is the same
//! one, outside every PID namespace a sandbox or container made.
//!
//! # A synchronised resize
//!
//! A window that lists `_NET_WM_SYNC_REQUEST` and names two XSync counters in
//! `_NET_WM_SYNC_REQUEST_COUNTER` is sent a request ahead of each resize the
//! window manager makes: a number, which the window sets its counter to once
//! it has repainted for the configure that follows. A window manager that
//! waits for the counter (mutter, KWin, xfwm4, openbox) draws no frame around
//! content of the old size and resizes no faster than the window repaints.
//! [`ResizeSync`] decides when each counter is set:
//!
//! - a request waits for the configure it precedes (a configure with no
//!   request before it is the window's own and answers nothing);
//! - at the window's next read of the server, a configure with no frame asked
//!   for (one that kept the size) is answered at once;
//! - otherwise the request is answered after the next frame the window
//!   delivers, whatever its handler did with it, so a handler that declines,
//!   fails or panics never leaves the window manager waiting.
//!
//! The second counter is the extended one of the frame synchronisation a
//! window manager offers by listing `_NET_WM_FRAME_DRAWN`: odd while the
//! window draws a frame, so the compositor never shows one half drawn, even
//! once it is drawn. A request the window manager marks extended is answered
//! on it, at an even value no lower than the request's, as GDK does
//! (`gdk_x11_surface_begin_frame`); the counter never rests above the largest
//! even value, `i64::MAX - 1`.
//!
//! A frame is answered once its handler has returned, which is when it has
//! been handed to the GPU, not when it is shown: the X11 host hears no
//! presentation (the Present extension tells the GPU driver's connection), and
//! with a FIFO swap chain the driver presents at a later refresh.

#![forbid(unsafe_code)]

use std::num::NonZeroI64;

/// The value of an XSync counter: the SYNC extension's signed 64-bit `INT64`,
/// which window managers compare as signed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CounterValue(i64);

impl CounterValue {
    /// The value whose low and high 32 bits are `low` and `high`, as a
    /// `_NET_WM_SYNC_REQUEST` message carries it.
    pub(crate) fn from_words(low: u32, high: u32) -> Self {
        Self(((u64::from(high) << 32) | u64::from(low)).cast_signed())
    }

    /// The value's high and low halves, as the SYNC extension's `INT64` holds
    /// them.
    pub(crate) fn halves(self) -> (i32, u32) {
        let high = i32::try_from(self.0 >> 32).expect("BUG: the high half of an i64 fits an i32");
        let low = u32::try_from(self.0 & 0xFFFF_FFFF).expect("BUG: a masked low half fits a u32");
        (high, low)
    }

    /// The largest even value: an odd one says "drawing" to a window manager
    /// that synchronises frames, so the extended counter never rests above it.
    const LARGEST_EVEN: Self = Self(i64::MAX - 1);

    /// The odd value a frame begins at, after this even one, which is at most
    /// [`LARGEST_EVEN`](Self::LARGEST_EVEN), so the sum cannot overflow.
    const fn odd_after(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    /// The even value a frame ends at, after this odd one: the largest even
    /// value where no larger one is.
    const fn even_after(self) -> Self {
        Self(self.0.saturating_add(1)).even_at_least()
    }

    /// The smallest even value no lower than this one, or the largest even
    /// value where none is: a window manager that asks for `i64::MAX` is
    /// answered with the value below it.
    const fn even_at_least(self) -> Self {
        if self.0 % 2 == 0 {
            self
        } else {
            match self.0.checked_add(1) {
                Some(even) => Self(even),
                None => Self::LARGEST_EVEN,
            }
        }
    }

    /// Whether the value is odd: an extended counter that is says the window
    /// is drawing a frame.
    #[cfg(test)]
    const fn is_odd(self) -> bool {
        self.0 % 2 != 0
    }
}

/// One `_NET_WM_SYNC_REQUEST` (EWMH): the value the window manager waits for,
/// and which counter it waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SyncRequest {
    value: NonZeroI64,
    /// The window manager waits on the extended counter.
    extended: bool,
}

impl SyncRequest {
    /// The request a `_NET_WM_SYNC_REQUEST` message carries in its five
    /// 32-bit words: the protocol atom, a timestamp, the value's low and high
    /// words, and a word that is not zero when the window manager waits on the
    /// extended counter. `None` for the value 0, which the protocol forbids.
    pub(crate) fn from_message(data: [u32; 5]) -> Option<Self> {
        let value = CounterValue::from_words(data[2], data[3]).0;
        Some(Self {
            value: NonZeroI64::new(value)?,
            extended: data[4] != 0,
        })
    }

    fn value(self) -> CounterValue {
        CounterValue(self.value.get())
    }
}

/// What a window sets its counters to: nothing, either, or both.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CounterWrites {
    /// The value for the counter a window manager without frame
    /// synchronisation waits on.
    pub basic: Option<CounterValue>,
    /// The value for the extended counter.
    pub extended: Option<CounterValue>,
}

/// A request a configure followed, owed its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owed {
    /// The window has not read the server since the configure.
    AtRead(SyncRequest),
    /// The read asked for a frame: the request is answered after it.
    AfterFrame(SyncRequest),
}

impl Owed {
    const fn request(self) -> SyncRequest {
        match self {
            Self::AtRead(request) | Self::AfterFrame(request) => request,
        }
    }
}

/// When a window sets the counters a synchronised resize waits on (see the
/// module). Owner-thread state of one window.
#[derive(Debug, Default)]
pub(crate) struct ResizeSync {
    /// The last request heard, waiting for the configure it precedes; a
    /// later one replaces it (EWMH: the counter takes the last value).
    requested: Option<SyncRequest>,
    /// The request of the last configure, not answered yet.
    owed: Option<Owed>,
    /// The extended counter's value as last set: even outside a frame.
    extended: CounterValue,
}

/// A frame the window delivers, between [`ResizeSync::frame_begins`] and
/// [`ResizeSync::frame_ends`]: what its end answers.
#[derive(Debug)]
#[must_use = "a frame begun is ended, or the window manager waits on its counter"]
pub(crate) struct FrameSync {
    answers: Option<SyncRequest>,
    /// The extended counter was made odd as the frame began.
    bracketed: bool,
}

impl ResizeSync {
    /// The window manager sent `request` ahead of a resize.
    pub(crate) fn request(&mut self, request: SyncRequest) {
        self.requested = Some(request);
    }

    /// A configure reached the window: the request before it, if any, is owed
    /// once the window has repainted for it, and replaces one still owed.
    pub(crate) fn configured(&mut self) {
        if let Some(request) = self.requested.take() {
            self.owed = Some(Owed::AtRead(request));
        }
    }

    /// The window read the server after a configure: with a frame asked for
    /// (`frame_asked`), its request waits for that frame; with none, there is
    /// nothing to repaint and it is answered now.
    pub(crate) fn read(&mut self, frame_asked: bool) -> CounterWrites {
        match self.owed {
            Some(Owed::AtRead(request)) if frame_asked => {
                self.owed = Some(Owed::AfterFrame(request));
                CounterWrites::default()
            }
            Some(Owed::AtRead(request)) => {
                self.owed = None;
                self.answer(request)
            }
            Some(Owed::AfterFrame(_)) | None => CounterWrites::default(),
        }
    }

    /// The window is about to deliver a frame: it answers the request owed,
    /// and, where the window manager synchronises frames (`paced`) or waits on
    /// the extended counter, the extended counter turns odd until the frame
    /// ends.
    pub(crate) fn frame_begins(&mut self, paced: bool) -> (FrameSync, CounterWrites) {
        let answers = self.owed.take().map(Owed::request);
        let extended = answers.filter(|request| request.extended);
        let bracketed = paced || extended.is_some();
        let mut writes = CounterWrites::default();
        if bracketed {
            if let Some(request) = extended {
                self.extended = self.extended.max(request.value().even_at_least());
            }
            self.extended = self.extended.odd_after();
            writes.extended = Some(self.extended);
        }
        (FrameSync { answers, bracketed }, writes)
    }

    /// The frame `frame` ended, however its handler returned: the extended
    /// counter turns even again, and the request the frame repainted for is
    /// answered.
    pub(crate) fn frame_ends(&mut self, frame: FrameSync) -> CounterWrites {
        let mut writes = CounterWrites::default();
        if frame.bracketed {
            self.extended = self.extended.even_after();
            writes.extended = Some(self.extended);
        }
        if let Some(request) = frame.answers.filter(|request| !request.extended) {
            writes.basic = Some(request.value());
        }
        writes
    }

    /// Answer `request` outside a frame.
    fn answer(&mut self, request: SyncRequest) -> CounterWrites {
        if request.extended {
            self.extended = self.extended.max(request.value().even_at_least());
            CounterWrites {
                basic: None,
                extended: Some(self.extended),
            }
        } else {
            CounterWrites {
                basic: Some(request.value()),
                extended: None,
            }
        }
    }
}

/// How a [`ClientMachine`]'s name is encoded, which names the type of the
/// property it is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameEncoding {
    /// ASCII, written as `STRING` (Latin-1, of which ASCII is the lower half).
    Latin1,
    /// Other UTF-8, written as `UTF8_STRING`, which `STRING` cannot carry.
    Utf8,
}

/// The name of the machine a window's process runs on, as
/// `WM_CLIENT_MACHINE` carries it (ICCCM §4.1.2.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClientMachine {
    name: Box<[u8]>,
    encoding: NameEncoding,
}

impl ClientMachine {
    /// The machine named `name`, the node name `uname` reports. `None` for an
    /// empty name, or one with a control character, a space or bytes that are
    /// not UTF-8: no window manager could match it against a machine, and a
    /// window then writes no `_NET_WM_PID` either ([`ClientProcess`]).
    pub(crate) fn from_node_name(name: &[u8]) -> Option<Self> {
        let encoding = if name.iter().all(u8::is_ascii_graphic) {
            NameEncoding::Latin1
        } else {
            let text = std::str::from_utf8(name).ok()?;
            if text
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
            {
                return None;
            }
            NameEncoding::Utf8
        };
        (!name.is_empty()).then(|| Self {
            name: name.into(),
            encoding,
        })
    }

    /// The name's bytes.
    pub(crate) fn name(&self) -> &[u8] {
        &self.name
    }

    /// How the name is encoded.
    pub(crate) const fn encoding(&self) -> NameEncoding {
        self.encoding
    }
}

/// The inode of the initial PID namespace (`PROC_PID_INIT_INO`, Linux 3.8),
/// which `/proc/self/ns/pid` names for a process outside every PID namespace
/// a sandbox or container made.
const INITIAL_PID_NAMESPACE: u64 = 0xEFFF_FFFC;

/// What a window says of its process: the machine it runs on, and its id,
/// where a window manager on that machine can signal it by that id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClientProcess {
    machine: Option<ClientMachine>,
    id: Option<std::num::NonZeroU32>,
}

impl ClientProcess {
    /// The process `id`, on the machine `node_name` names (what `uname`
    /// reports), in the PID namespace `/proc/self/ns/pid` names (its link,
    /// `pid:[<inode>]`; `None` where it cannot be read).
    ///
    /// The id is kept only beside a machine (EWMH) and only in the initial PID
    /// namespace: a window manager kills a window that stopped answering its
    /// pings by this id, and inside a sandbox's or container's namespace
    /// (Flatpak, a container, a WSL distribution) the id names another
    /// process outside it. The `NSpid` line of `/proc/self/status` cannot
    /// tell: a sandbox that mounts its own `/proc`, as Flatpak's does, lists
    /// one id there. A namespace that cannot be read keeps no id either, and
    /// a window manager inside the same namespace loses only its offer to end
    /// the process.
    pub(crate) fn read(node_name: &[u8], namespace: Option<&str>, id: u32) -> Self {
        let machine = ClientMachine::from_node_name(node_name);
        let initial = namespace.and_then(namespace_inode) == Some(INITIAL_PID_NAMESPACE);
        let id = machine
            .as_ref()
            .filter(|_| initial)
            .and_then(|_| std::num::NonZeroU32::new(id));
        Self { machine, id }
    }

    /// The machine a window names in `WM_CLIENT_MACHINE`.
    pub(crate) const fn machine(&self) -> Option<&ClientMachine> {
        self.machine.as_ref()
    }

    /// The id a window names in `_NET_WM_PID`, only beside its machine.
    pub(crate) const fn id(&self) -> Option<std::num::NonZeroU32> {
        self.id
    }
}

/// The inode a namespace link (`pid:[4026531836]`) names.
fn namespace_inode(link: &str) -> Option<u64> {
    link.strip_prefix("pid:[")?
        .strip_suffix(']')?
        .parse::<u64>()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(raw: i64) -> CounterValue {
        CounterValue(raw)
    }

    fn request(raw: i64, extended: bool) -> SyncRequest {
        SyncRequest {
            value: NonZeroI64::new(raw).expect("a request's value is not zero"),
            extended,
        }
    }

    /// A request's value is the 64-bit number its two words make, signed as
    /// XSync compares it, and splits back into the halves the extension
    /// takes; a value of 0 is no request.
    #[test]
    fn a_request_carries_the_value_of_its_two_words() {
        // (case, low and high words, the value)
        let table = [
            ("one", (1, 0), 1),
            ("the high word", (5, 1), (1 << 32) + 5),
            ("the low word's top bit", (0x8000_0000, 0), 0x8000_0000),
            ("negative", (u32::MAX, u32::MAX), -1),
            ("the largest", (u32::MAX, 0x7FFF_FFFF), i64::MAX),
            ("the smallest", (0, 0x8000_0000), i64::MIN),
        ];
        for (case, (low, high), raw) in table {
            let read = CounterValue::from_words(low, high);
            assert_eq!(read, value(raw), "{case}");
            let (back_high, back_low) = read.halves();
            assert_eq!(
                (back_low, back_high.cast_unsigned()),
                (low, high),
                "{case}: the halves"
            );
            let parsed = SyncRequest::from_message([0, 0, low, high, 1])
                .expect("a value that is not zero is a request");
            assert_eq!(parsed, request(raw, true), "{case}: the message");
        }
        assert_eq!(SyncRequest::from_message([0, 0, 0, 0, 0]), None);
        assert_eq!(
            SyncRequest::from_message([0, 0, 7, 0, 0]),
            Some(request(7, false)),
            "a fifth word of 0 is the basic counter"
        );
    }

    /// When a request is answered: after the configure it precedes, at the
    /// read when no frame is asked for and after the next frame otherwise,
    /// with the last request's value; a configure without a request, and a
    /// request without a configure, answer nothing.
    #[test]
    fn a_request_is_answered_once_its_configure_is_repainted() {
        enum Step {
            Request(i64),
            Configure,
            Read { frame_asked: bool },
            Frame,
        }
        use Step::{Configure, Frame, Read, Request};
        // (case, steps, the basic counter's writes in order)
        let table: &[(&str, &[Step], &[i64])] = &[
            (
                "a resize: answered after its frame",
                &[Request(4), Configure, Read { frame_asked: true }, Frame],
                &[4],
            ),
            (
                "a configure that kept the size: answered at the read",
                &[Request(4), Configure, Read { frame_asked: false }, Frame],
                &[4],
            ),
            (
                "nothing before the frame",
                &[Request(4), Configure, Read { frame_asked: true }],
                &[],
            ),
            (
                "a request with no configure yet",
                &[Request(4), Read { frame_asked: false }, Frame],
                &[],
            ),
            (
                "a configure of the window's own",
                &[Configure, Read { frame_asked: false }, Frame],
                &[],
            ),
            (
                "two pairs before the read: the last value",
                &[
                    Request(4),
                    Configure,
                    Request(5),
                    Configure,
                    Read { frame_asked: true },
                    Frame,
                ],
                &[5],
            ),
            (
                "two requests before one configure: the last value",
                &[
                    Request(4),
                    Request(5),
                    Configure,
                    Read { frame_asked: false },
                ],
                &[5],
            ),
            (
                "a frame delivered before the read repaints for the configure",
                &[Request(4), Configure, Frame],
                &[4],
            ),
            (
                "answered once",
                &[
                    Request(4),
                    Configure,
                    Read { frame_asked: true },
                    Frame,
                    Read { frame_asked: false },
                    Frame,
                ],
                &[4],
            ),
            (
                "a request heard while one waits for its frame",
                &[
                    Request(4),
                    Configure,
                    Read { frame_asked: true },
                    Request(5),
                    Frame,
                    Configure,
                    Read { frame_asked: false },
                ],
                &[4, 5],
            ),
        ];
        for (case, steps, expected) in table {
            let mut sync = ResizeSync::default();
            let mut written = Vec::new();
            for step in *steps {
                let writes = match step {
                    Request(raw) => {
                        sync.request(request(*raw, false));
                        CounterWrites::default()
                    }
                    Configure => {
                        sync.configured();
                        CounterWrites::default()
                    }
                    Read { frame_asked } => sync.read(*frame_asked),
                    Frame => {
                        let (frame, begun) = sync.frame_begins(false);
                        assert_eq!(begun, CounterWrites::default(), "{case}: an unpaced frame");
                        sync.frame_ends(frame)
                    }
                };
                assert_eq!(writes.extended, None, "{case}: the basic counter alone");
                written.extend(writes.basic);
            }
            let expected: Vec<_> = expected.iter().copied().map(value).collect();
            assert_eq!(written, expected, "{case}");
        }
    }

    /// Under a window manager that synchronises frames, the extended counter
    /// is odd while each frame draws and even after; an extended request is
    /// answered on it at an even value no lower than its own, after its frame
    /// or at the read, and never on the basic counter.
    #[test]
    fn the_extended_counter_brackets_each_frame() {
        let frame = |sync: &mut ResizeSync, paced: bool| {
            let (frame, begun) = sync.frame_begins(paced);
            let ended = sync.frame_ends(frame);
            assert_eq!(begun.basic, None);
            (begun.extended, ended)
        };

        let mut sync = ResizeSync::default();
        let (begun, ended) = frame(&mut sync, true);
        assert_eq!(begun, Some(value(1)), "odd as the frame draws");
        assert_eq!(ended.extended, Some(value(2)), "even once drawn");
        assert_eq!(frame(&mut sync, false), (None, CounterWrites::default()));

        // (case, the request, frame asked at the read, the extended value
        //  answered: during the frame and after it)
        let table = [
            ("an even request, repainted", 10, true, Some(11), 12),
            ("an odd request, repainted", 11, true, Some(13), 14),
            ("below the counter", 1, true, Some(3), 4),
            ("an even request at the read", 10, false, None, 10),
            ("an odd request at the read", 11, false, None, 12),
            // No even value is above `i64::MAX - 1`, and an odd one says
            // "drawing": the counter rests there.
            (
                "the largest, repainted",
                i64::MAX,
                true,
                Some(i64::MAX),
                i64::MAX - 1,
            ),
            (
                "the largest at the read",
                i64::MAX,
                false,
                None,
                i64::MAX - 1,
            ),
        ];
        for (case, raw, frame_asked, during, after) in table {
            let mut sync = ResizeSync::default();
            let _ = frame(&mut sync, true);
            sync.request(request(raw, true));
            sync.configured();
            let at_read = sync.read(frame_asked);
            if frame_asked {
                assert_eq!(at_read, CounterWrites::default(), "{case}: at the read");
                // A window manager that waits on the extended counter
                // brackets the frame though it lists no frame drawn.
                let (begun, ended) = frame(&mut sync, false);
                assert_eq!(begun, during.map(value), "{case}: during the frame");
                assert!(begun.is_some_and(CounterValue::is_odd), "{case}");
                assert_eq!(ended.basic, None, "{case}: not on the basic counter");
                assert_eq!(ended.extended, Some(value(after)), "{case}: after it");
            } else {
                assert_eq!(
                    at_read,
                    CounterWrites {
                        basic: None,
                        extended: Some(value(after)),
                    },
                    "{case}: at the read"
                );
            }
        }
    }

    /// A node name is written as the text type that carries it, and a name no
    /// window manager could match against a machine is not written.
    #[test]
    fn a_machine_name_is_written_in_the_type_that_carries_it() {
        // (case, the node name, its encoding)
        let table: &[(&str, &[u8], Option<NameEncoding>)] = &[
            (
                "a host",
                b"build-07.example.org",
                Some(NameEncoding::Latin1),
            ),
            ("UTF-8", "h\u{f6}st".as_bytes(), Some(NameEncoding::Utf8)),
            ("empty", b"", None),
            ("a space", b"my host", None),
            ("a control character", b"host\n", None),
            ("not UTF-8", b"h\xF6st", None),
        ];
        for (case, name, encoding) in table {
            let machine = ClientMachine::from_node_name(name);
            assert_eq!(
                machine.as_ref().map(ClientMachine::encoding),
                *encoding,
                "{case}"
            );
            if let Some(machine) = machine {
                assert_eq!(machine.name(), *name, "{case}: the bytes");
            }
        }
    }

    /// A process id is written only beside a machine and only from the
    /// initial PID namespace, where a window manager's id for the process is
    /// the same one; anywhere else, or where the namespace cannot be read, the
    /// machine alone is.
    #[test]
    fn a_process_id_is_written_only_where_a_window_manager_can_use_it() {
        const INITIAL: &str = "pid:[4026531836]";
        // (case, node name, the namespace link, the machine written, the id
        //  written)
        type Case = (
            &'static str,
            &'static [u8],
            Option<&'static str>,
            bool,
            Option<u32>,
        );
        let table: &[Case] = &[
            (
                "the initial namespace",
                b"host",
                Some(INITIAL),
                true,
                Some(4242),
            ),
            (
                "a sandbox's namespace",
                b"host",
                Some("pid:[4026532223]"),
                true,
                None,
            ),
            ("an unreadable namespace", b"host", None, true, None),
            (
                "a link of another kind",
                b"host",
                Some("net:[4026531836]"),
                true,
                None,
            ),
            ("a malformed link", b"host", Some("pid:[x]"), true, None),
            ("no machine", b"", Some(INITIAL), false, None),
        ];
        for (case, name, namespace, machine, id) in table {
            let process = ClientProcess::read(name, *namespace, 4242);
            assert_eq!(process.machine().is_some(), *machine, "{case}: the machine");
            assert_eq!(
                process.id().map(std::num::NonZeroU32::get),
                *id,
                "{case}: the id"
            );
        }
    }
}
