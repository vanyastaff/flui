//! The X Input Method protocol's messages as bytes (the X Consortium's
//! "The Input Method Protocol", version 1.0), both ways.
//!
//! A message is a four-byte header (major opcode, minor opcode, the body's
//! length in four-byte units) and its body, in the byte order the client
//! named in `XIM_CONNECT`; this client names its own, so both ends speak
//! native order. A server's bytes are untrusted: every length is checked
//! against what is left before it is read, a message is at most
//! [`MAX_MESSAGE`] bytes, and a message this client has no use for decodes as
//! [`Message::Other`] without its body being read.

/// The longest message read or written, header included. The longest a
/// server sends in practice is a preedit draw of a few hundred characters.
pub(crate) const MAX_MESSAGE: usize = 64 * 1024;

/// A key or button event as `XIM_FORWARD_EVENT` carries it: the 32 bytes of
/// the X protocol's event, in the client's byte order.
pub(crate) type WireEvent = [u8; 32];

/// The X protocol's `KeyPress` event code.
pub(crate) const KEY_PRESS: u8 = 2;
/// The X protocol's `KeyRelease` event code.
pub(crate) const KEY_RELEASE: u8 = 3;

/// `XIM_FORWARD_EVENT`'s flag: the receiver answers `XIM_SYNC_REPLY` once it
/// handled the event.
pub(crate) const FORWARD_SYNCHRONOUS: u16 = 0x0001;

/// `XIM_COMMIT`'s flags.
const COMMIT_SYNCHRONOUS: u16 = 0x0001;
const COMMIT_CHARS: u16 = 0x0002;
const COMMIT_KEYSYM: u16 = 0x0004;

/// `XIM_PREEDIT_DRAW`'s status: no string, the preedit is empty.
pub(crate) const PREEDIT_NO_STRING: u32 = 0x0001;

/// `XIM_ERROR`'s flag bits: which of its ids are valid.
#[cfg(test)]
pub(crate) const ERROR_IM_VALID: u16 = 0x0001;
pub(crate) const ERROR_IC_VALID: u16 = 0x0002;

/// An attribute a server offers in `XIM_OPEN_REPLY`: its id and its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AttrDecl {
    pub(crate) id: u16,
    pub(crate) kind: u16,
    pub(crate) name: Vec<u8>,
}

/// An attribute's value, by id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AttrValue {
    pub(crate) id: u16,
    pub(crate) value: Vec<u8>,
}

/// One message of the protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Message {
    Connect,
    ConnectReply,
    Disconnect,
    DisconnectReply,
    Error {
        im: u16,
        ic: u16,
        flag: u16,
        code: u16,
    },
    Open {
        locale: Vec<u8>,
    },
    OpenReply {
        im: u16,
        im_attrs: Vec<AttrDecl>,
        ic_attrs: Vec<AttrDecl>,
    },
    Close {
        im: u16,
    },
    CloseReply {
        im: u16,
    },
    SetEventMask {
        im: u16,
        ic: u16,
        forward: u32,
        synchronous: u32,
    },
    /// Asks for COMPOUND_TEXT, the one encoding every server speaks.
    EncodingNegotiation {
        im: u16,
    },
    EncodingNegotiationReply {
        im: u16,
        index: i16,
    },
    GetImValues {
        im: u16,
        ids: Vec<u16>,
    },
    GetImValuesReply {
        im: u16,
        values: Vec<AttrValue>,
    },
    CreateIc {
        im: u16,
        values: Vec<AttrValue>,
    },
    CreateIcReply {
        im: u16,
        ic: u16,
    },
    DestroyIc {
        im: u16,
        ic: u16,
    },
    DestroyIcReply {
        im: u16,
        ic: u16,
    },
    SetIcValues {
        im: u16,
        ic: u16,
        values: Vec<AttrValue>,
    },
    SetIcValuesReply {
        im: u16,
        ic: u16,
    },
    SetIcFocus {
        im: u16,
        ic: u16,
    },
    UnsetIcFocus {
        im: u16,
        ic: u16,
    },
    ForwardEvent {
        im: u16,
        ic: u16,
        flag: u16,
        serial: u16,
        event: WireEvent,
    },
    Sync {
        im: u16,
        ic: u16,
    },
    SyncReply {
        im: u16,
        ic: u16,
    },
    /// Committed text (COMPOUND_TEXT) or a keysym; `synchronous` asks for
    /// `XIM_SYNC_REPLY`.
    Commit {
        im: u16,
        ic: u16,
        synchronous: bool,
        keysym: Option<u32>,
        text: Option<Vec<u8>>,
    },
    ResetIc {
        im: u16,
        ic: u16,
    },
    ResetIcReply {
        im: u16,
        ic: u16,
        text: Vec<u8>,
    },
    PreeditStart {
        im: u16,
        ic: u16,
    },
    PreeditStartReply {
        im: u16,
        ic: u16,
        limit: i32,
    },
    /// Characters `first..first + length` of the preedit become `text`
    /// (COMPOUND_TEXT), the caret at character `caret`.
    PreeditDraw {
        im: u16,
        ic: u16,
        caret: i32,
        first: i32,
        length: i32,
        status: u32,
        text: Vec<u8>,
    },
    PreeditCaret {
        im: u16,
        ic: u16,
        position: i32,
        direction: u32,
        style: u32,
    },
    PreeditCaretReply {
        im: u16,
        ic: u16,
        position: i32,
    },
    PreeditDone {
        im: u16,
        ic: u16,
    },
    /// A message this client does not use, by its major opcode.
    Other(u8),
}

/// Why a message could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum WireError {
    #[error("the message is shorter than its header or its own length says")]
    Truncated,
    #[error("the message is longer than any this client reads")]
    TooLong,
}

impl Message {
    /// The message as bytes, header included.
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = Out::new();
        let major = match self {
            Self::Connect => {
                out.u8(if cfg!(target_endian = "little") {
                    0x6c
                } else {
                    0x42
                });
                out.u8(0);
                // Protocol 1.0, no authentication.
                out.u16(1);
                out.u16(0);
                out.u16(0);
                1
            }
            Self::ConnectReply => {
                out.u16(1);
                out.u16(0);
                2
            }
            Self::Disconnect => 3,
            Self::DisconnectReply => 4,
            Self::Error { im, ic, flag, code } => {
                out.u16(*im);
                out.u16(*ic);
                out.u16(*flag);
                out.u16(*code);
                out.u16(0);
                out.u16(0);
                20
            }
            Self::Open { locale } => {
                out.u8(u8::try_from(locale.len()).unwrap_or(u8::MAX));
                out.bytes(&locale[..locale.len().min(usize::from(u8::MAX))]);
                out.pad4();
                30
            }
            Self::OpenReply {
                im,
                im_attrs,
                ic_attrs,
            } => {
                out.u16(*im);
                out.counted(|out| {
                    for attr in im_attrs {
                        out.decl(attr);
                    }
                });
                out.counted_unused(|out| {
                    for attr in ic_attrs {
                        out.decl(attr);
                    }
                });
                31
            }
            Self::Close { im } => {
                out.u16(*im);
                out.u16(0);
                32
            }
            Self::CloseReply { im } => {
                out.u16(*im);
                out.u16(0);
                33
            }
            Self::SetEventMask {
                im,
                ic,
                forward,
                synchronous,
            } => {
                out.u16(*im);
                out.u16(*ic);
                out.u32(*forward);
                out.u32(*synchronous);
                37
            }
            Self::EncodingNegotiation { im } => {
                out.u16(*im);
                out.counted(|out| {
                    let name = b"COMPOUND_TEXT";
                    out.u8(name.len() as u8);
                    out.bytes(name);
                });
                out.pad4();
                // No encoding information.
                out.u16(0);
                out.u16(0);
                38
            }
            Self::EncodingNegotiationReply { im, index } => {
                out.u16(*im);
                // The category: by name.
                out.u16(0);
                out.i16(*index);
                out.u16(0);
                39
            }
            Self::GetImValues { im, ids } => {
                out.u16(*im);
                out.counted(|out| {
                    for id in ids {
                        out.u16(*id);
                    }
                });
                out.pad4();
                44
            }
            Self::GetImValuesReply { im, values } => {
                out.u16(*im);
                out.counted(|out| out.values(values));
                45
            }
            Self::CreateIc { im, values } => {
                out.u16(*im);
                out.counted(|out| out.values(values));
                50
            }
            Self::CreateIcReply { im, ic } => out.ids(*im, *ic, 51),
            Self::DestroyIc { im, ic } => out.ids(*im, *ic, 52),
            Self::DestroyIcReply { im, ic } => out.ids(*im, *ic, 53),
            Self::SetIcValues { im, ic, values } => {
                out.u16(*im);
                out.u16(*ic);
                out.counted_unused(|out| out.values(values));
                54
            }
            Self::SetIcValuesReply { im, ic } => out.ids(*im, *ic, 55),
            Self::SetIcFocus { im, ic } => out.ids(*im, *ic, 58),
            Self::UnsetIcFocus { im, ic } => out.ids(*im, *ic, 59),
            Self::ForwardEvent {
                im,
                ic,
                flag,
                serial,
                event,
            } => {
                out.u16(*im);
                out.u16(*ic);
                out.u16(*flag);
                out.u16(*serial);
                out.bytes(event);
                60
            }
            Self::Sync { im, ic } => out.ids(*im, *ic, 61),
            Self::SyncReply { im, ic } => out.ids(*im, *ic, 62),
            Self::Commit {
                im,
                ic,
                synchronous,
                keysym,
                text,
            } => {
                out.u16(*im);
                out.u16(*ic);
                let mut flag = if *synchronous { COMMIT_SYNCHRONOUS } else { 0 };
                if keysym.is_some() {
                    flag |= COMMIT_KEYSYM;
                }
                if text.is_some() {
                    flag |= COMMIT_CHARS;
                }
                out.u16(flag);
                if let Some(keysym) = keysym {
                    out.u16(0);
                    out.u32(*keysym);
                }
                if let Some(text) = text {
                    out.string16(text);
                    out.pad4();
                }
                63
            }
            Self::ResetIc { im, ic } => out.ids(*im, *ic, 64),
            Self::ResetIcReply { im, ic, text } => {
                out.u16(*im);
                out.u16(*ic);
                out.string16(text);
                out.pad4();
                65
            }
            Self::PreeditStart { im, ic } => out.ids(*im, *ic, 73),
            Self::PreeditStartReply { im, ic, limit } => {
                out.u16(*im);
                out.u16(*ic);
                out.i32(*limit);
                74
            }
            Self::PreeditDraw {
                im,
                ic,
                caret,
                first,
                length,
                status,
                text,
            } => {
                out.u16(*im);
                out.u16(*ic);
                out.i32(*caret);
                out.i32(*first);
                out.i32(*length);
                out.u32(*status);
                out.string16(text);
                out.pad4();
                // No feedback.
                out.u16(0);
                out.u16(0);
                75
            }
            Self::PreeditCaret {
                im,
                ic,
                position,
                direction,
                style,
            } => {
                out.u16(*im);
                out.u16(*ic);
                out.i32(*position);
                out.u32(*direction);
                out.u32(*style);
                76
            }
            Self::PreeditCaretReply { im, ic, position } => {
                out.u16(*im);
                out.u16(*ic);
                out.i32(*position);
                77
            }
            Self::PreeditDone { im, ic } => out.ids(*im, *ic, 78),
            Self::Other(major) => *major,
        };
        out.finish(major)
    }

    /// Read one message from `bytes`, which hold it whole; bytes past its
    /// length are ignored, as the transport may pad it.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        if bytes.len() > MAX_MESSAGE {
            return Err(WireError::TooLong);
        }
        let mut header = In::new(bytes, 0);
        let major = header.u8()?;
        let _minor = header.u8()?;
        let units = usize::from(header.u16()?);
        let end = 4 + units * 4;
        let body = bytes.get(4..end).ok_or(WireError::Truncated)?;
        let mut r = In::new(body, 4);
        let ids = |r: &mut In<'_>| -> Result<(u16, u16), WireError> { Ok((r.u16()?, r.u16()?)) };
        Ok(match major {
            1 => Self::Connect,
            2 => Self::ConnectReply,
            3 => Self::Disconnect,
            4 => Self::DisconnectReply,
            20 => {
                let (im, ic) = ids(&mut r)?;
                Self::Error {
                    im,
                    ic,
                    flag: r.u16()?,
                    code: r.u16()?,
                }
            }
            30 => {
                let len = usize::from(r.u8()?);
                Self::Open {
                    locale: r.take(len)?.to_vec(),
                }
            }
            31 => {
                let im = r.u16()?;
                let im_attrs = r.counted(In::decl)?;
                let ic_attrs = r.counted_unused(In::decl)?;
                Self::OpenReply {
                    im,
                    im_attrs,
                    ic_attrs,
                }
            }
            32 => Self::Close { im: r.u16()? },
            33 => Self::CloseReply { im: r.u16()? },
            37 => {
                let (im, ic) = ids(&mut r)?;
                Self::SetEventMask {
                    im,
                    ic,
                    forward: r.u32()?,
                    synchronous: r.u32()?,
                }
            }
            38 => Self::EncodingNegotiation { im: r.u16()? },
            39 => {
                let im = r.u16()?;
                let _category = r.u16()?;
                Self::EncodingNegotiationReply {
                    im,
                    index: r.i16()?,
                }
            }
            44 => {
                let im = r.u16()?;
                let ids = r.counted(In::u16)?;
                Self::GetImValues { im, ids }
            }
            45 => {
                let im = r.u16()?;
                let values = r.counted(In::value)?;
                Self::GetImValuesReply { im, values }
            }
            50 => {
                let im = r.u16()?;
                let values = r.counted(In::value)?;
                Self::CreateIc { im, values }
            }
            51 => {
                let (im, ic) = ids(&mut r)?;
                Self::CreateIcReply { im, ic }
            }
            52 => {
                let (im, ic) = ids(&mut r)?;
                Self::DestroyIc { im, ic }
            }
            53 => {
                let (im, ic) = ids(&mut r)?;
                Self::DestroyIcReply { im, ic }
            }
            54 => {
                let (im, ic) = ids(&mut r)?;
                let values = r.counted_unused(In::value)?;
                Self::SetIcValues { im, ic, values }
            }
            55 => {
                let (im, ic) = ids(&mut r)?;
                Self::SetIcValuesReply { im, ic }
            }
            58 => {
                let (im, ic) = ids(&mut r)?;
                Self::SetIcFocus { im, ic }
            }
            59 => {
                let (im, ic) = ids(&mut r)?;
                Self::UnsetIcFocus { im, ic }
            }
            60 => {
                let (im, ic) = ids(&mut r)?;
                let flag = r.u16()?;
                let serial = r.u16()?;
                let event = r
                    .take(32)?
                    .try_into()
                    .expect("BUG: 32 bytes were taken for the event");
                Self::ForwardEvent {
                    im,
                    ic,
                    flag,
                    serial,
                    event,
                }
            }
            61 => {
                let (im, ic) = ids(&mut r)?;
                Self::Sync { im, ic }
            }
            62 => {
                let (im, ic) = ids(&mut r)?;
                Self::SyncReply { im, ic }
            }
            63 => {
                let (im, ic) = ids(&mut r)?;
                let flag = r.u16()?;
                let keysym = if flag & COMMIT_KEYSYM == 0 {
                    None
                } else {
                    let _unused = r.u16()?;
                    Some(r.u32()?)
                };
                let text = if flag & COMMIT_CHARS == 0 {
                    None
                } else {
                    Some(r.string16()?)
                };
                Self::Commit {
                    im,
                    ic,
                    synchronous: flag & COMMIT_SYNCHRONOUS != 0,
                    keysym,
                    text,
                }
            }
            64 => {
                let (im, ic) = ids(&mut r)?;
                Self::ResetIc { im, ic }
            }
            65 => {
                let (im, ic) = ids(&mut r)?;
                Self::ResetIcReply {
                    im,
                    ic,
                    text: r.string16()?,
                }
            }
            73 => {
                let (im, ic) = ids(&mut r)?;
                Self::PreeditStart { im, ic }
            }
            74 => {
                let (im, ic) = ids(&mut r)?;
                Self::PreeditStartReply {
                    im,
                    ic,
                    limit: r.i32()?,
                }
            }
            75 => {
                let (im, ic) = ids(&mut r)?;
                let caret = r.i32()?;
                let first = r.i32()?;
                let length = r.i32()?;
                let status = r.u32()?;
                // The feedback after it is not read: the field styles a
                // preedit its own way.
                let text = r.string16()?;
                Self::PreeditDraw {
                    im,
                    ic,
                    caret,
                    first,
                    length,
                    status,
                    text,
                }
            }
            76 => {
                let (im, ic) = ids(&mut r)?;
                Self::PreeditCaret {
                    im,
                    ic,
                    position: r.i32()?,
                    direction: r.u32()?,
                    style: r.u32()?,
                }
            }
            77 => {
                let (im, ic) = ids(&mut r)?;
                Self::PreeditCaretReply {
                    im,
                    ic,
                    position: r.i32()?,
                }
            }
            78 => {
                let (im, ic) = ids(&mut r)?;
                Self::PreeditDone { im, ic }
            }
            other => Self::Other(other),
        })
    }
}

/// A message being written: its body, after the header that `finish` puts
/// in front.
struct Out {
    body: Vec<u8>,
}

impl Out {
    fn new() -> Self {
        Self { body: Vec::new() }
    }

    fn u8(&mut self, value: u8) {
        self.body.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.body.extend_from_slice(&value.to_ne_bytes());
    }

    fn i16(&mut self, value: i16) {
        self.body.extend_from_slice(&value.to_ne_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.body.extend_from_slice(&value.to_ne_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.body.extend_from_slice(&value.to_ne_bytes());
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.body.extend_from_slice(bytes);
    }

    /// Pad to four bytes from the message's start, which the header's four
    /// bytes leave where the body's are.
    fn pad4(&mut self) {
        while !self.body.len().is_multiple_of(4) {
            self.body.push(0);
        }
    }

    /// A string with its two-byte length, cut at `u16::MAX` bytes.
    fn string16(&mut self, text: &[u8]) {
        let text = &text[..text.len().min(usize::from(u16::MAX))];
        self.u16(u16::try_from(text.len()).expect("BUG: the text was cut to u16::MAX bytes"));
        self.bytes(text);
    }

    /// A list after its byte length.
    fn counted(&mut self, list: impl FnOnce(&mut Self)) {
        let at = self.body.len();
        self.u16(0);
        let start = self.body.len();
        list(self);
        self.set_length(at, start);
    }

    /// A list after its byte length and two unused bytes.
    fn counted_unused(&mut self, list: impl FnOnce(&mut Self)) {
        let at = self.body.len();
        self.u16(0);
        self.u16(0);
        let start = self.body.len();
        list(self);
        self.set_length(at, start);
    }

    fn set_length(&mut self, at: usize, start: usize) {
        let length = u16::try_from(self.body.len() - start).unwrap_or(u16::MAX);
        self.body[at..at + 2].copy_from_slice(&length.to_ne_bytes());
    }

    fn decl(&mut self, attr: &AttrDecl) {
        self.u16(attr.id);
        self.u16(attr.kind);
        self.string16(&attr.name);
        self.pad4();
    }

    fn values(&mut self, values: &[AttrValue]) {
        for value in values {
            self.u16(value.id);
            self.string16(&value.value);
            self.pad4();
        }
    }

    fn ids(&mut self, im: u16, ic: u16, major: u8) -> u8 {
        self.u16(im);
        self.u16(ic);
        major
    }

    fn finish(mut self, major: u8) -> Vec<u8> {
        self.pad4();
        let units = u16::try_from(self.body.len() / 4).unwrap_or(u16::MAX);
        let mut message = Vec::with_capacity(4 + self.body.len());
        message.push(major);
        message.push(0);
        message.extend_from_slice(&units.to_ne_bytes());
        message.extend_from_slice(&self.body);
        message
    }
}

/// A message being read: what is left of its body, and the offset of the
/// body's next byte from the message's start, which padding counts from.
struct In<'b> {
    bytes: &'b [u8],
    offset: usize,
}

impl<'b> In<'b> {
    fn new(bytes: &'b [u8], offset: usize) -> Self {
        Self { bytes, offset }
    }

    fn take(&mut self, len: usize) -> Result<&'b [u8], WireError> {
        if self.bytes.len() < len {
            return Err(WireError::Truncated);
        }
        let (taken, rest) = self.bytes.split_at(len);
        self.bytes = rest;
        self.offset += len;
        Ok(taken)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        Ok(self
            .take(N)?
            .try_into()
            .expect("BUG: N bytes were taken for an array of N"))
    }

    fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, WireError> {
        Ok(u16::from_ne_bytes(self.array()?))
    }

    fn i16(&mut self) -> Result<i16, WireError> {
        Ok(i16::from_ne_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, WireError> {
        Ok(u32::from_ne_bytes(self.array()?))
    }

    fn i32(&mut self) -> Result<i32, WireError> {
        Ok(i32::from_ne_bytes(self.array()?))
    }

    /// Skip to four bytes from the message's start; a message may end
    /// without its last padding.
    fn pad4(&mut self) {
        let pad = (4 - self.offset % 4) % 4;
        let pad = pad.min(self.bytes.len());
        self.bytes = &self.bytes[pad..];
        self.offset += pad;
    }

    /// A string after its two-byte length, and its padding.
    fn string16(&mut self) -> Result<Vec<u8>, WireError> {
        let len = usize::from(self.u16()?);
        let text = self.take(len)?.to_vec();
        self.pad4();
        Ok(text)
    }

    /// Items of a list after its byte length, read from those bytes alone.
    fn counted<T>(
        &mut self,
        item: impl Fn(&mut In<'b>) -> Result<T, WireError>,
    ) -> Result<Vec<T>, WireError> {
        let len = usize::from(self.u16()?);
        self.list(len, item)
    }

    /// As [`In::counted`], with two unused bytes after the length.
    fn counted_unused<T>(
        &mut self,
        item: impl Fn(&mut In<'b>) -> Result<T, WireError>,
    ) -> Result<Vec<T>, WireError> {
        let len = usize::from(self.u16()?);
        let _unused = self.u16()?;
        self.list(len, item)
    }

    fn list<T>(
        &mut self,
        len: usize,
        item: impl Fn(&mut In<'b>) -> Result<T, WireError>,
    ) -> Result<Vec<T>, WireError> {
        let offset = self.offset;
        let mut list = In::new(self.take(len)?, offset);
        let mut items = Vec::new();
        while !list.bytes.is_empty() {
            items.push(item(&mut list)?);
        }
        Ok(items)
    }

    fn decl(&mut self) -> Result<AttrDecl, WireError> {
        let id = self.u16()?;
        let kind = self.u16()?;
        let name = self.string16()?;
        Ok(AttrDecl { id, kind, name })
    }

    fn value(&mut self) -> Result<AttrValue, WireError> {
        let id = self.u16()?;
        let value = self.string16()?;
        Ok(AttrValue { id, value })
    }
}

/// Attribute values nested in one, `preeditAttributes`'s: the bytes of its
/// value.
pub(crate) fn nested(values: &[AttrValue]) -> Vec<u8> {
    let mut out = Out::new();
    out.values(values);
    out.body
}

/// Text a server sent in COMPOUND_TEXT, as UTF-8; `None` for text that does
/// not decode.
pub(crate) fn text_of(compound: &[u8]) -> Option<String> {
    // `xim_ctext` shifts each byte of a GB 2312 or KS C 5601 run up by
    // 0x80, which overflows (and panics where overflow is checked) on a
    // byte already past it: such a run is not COMPOUND_TEXT's anyway.
    let overflows = compound.split(|&b| b == 0x1b).any(|run| {
        matches!(run, [0x24, 0x28, 0x41 | 0x43, rest @ ..] if rest.iter().any(|&b| b >= 0x80))
    });
    if overflows {
        return None;
    }
    xim_ctext::compound_text_to_utf8(compound).ok()
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
