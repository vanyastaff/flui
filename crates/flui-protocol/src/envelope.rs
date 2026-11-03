//! The lines of a conversation (ADR-0238 §2): what a client sends, a [`Greeting`] and then
//! [`Request`]s, and every [`ServerLine`] an app sends back.
//!
//! ```text
//! → {"hello":{"token":"…","protocol":"0.9","client":"flui-driver/0.1.0"}}   Greeting
//! ← {"hello":{"protocol":"0.9","app":{…},"capabilities":{…}}}               ServerLine::Hello
//! → {"id":7,"method":"semantics.locate","params":{…}}                       Request
//! ← {"id":7,"result":{…}}                                                   ServerLine::Answered
//! ← {"id":7,"error":{"code":…,"message":…,"retry":…}}                       ServerLine::Failed
//! ← {"error":{…}}                    a refusal that answers no request:     ServerLine::Failed
//! ← {"event":"frame","params":{…}}                                          ServerLine::Event
//! ```
//!
//! The types are generic over the parameters and the result, so each side picks its own
//! representation: a client writes a method's typed parameters, and a server reads them as its
//! JSON library's value until it knows the method. This crate names no JSON library.
//!
//! A [`ServerLine`] is told apart by its keys: `hello`, `event`, `error` or `result`, exactly one
//! of them. A line naming none, or several, is refused with a message that says so. An event line
//! names its `event` before its `params`, as every app writes it. Keys a reader does not know are
//! ignored, so a newer app's additions read in an older client.

use std::fmt;

use crate::error::ErrorObject;
use crate::events::Event;
use crate::session::{Hello, HelloReply};

/// A request's id, chosen by the client and echoed in its answer: a number or a string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(untagged)
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum RequestId {
    /// A number, as most clients count their requests.
    Number(u64),
    /// A string.
    Text(String),
}

impl From<u64> for RequestId {
    fn from(id: u64) -> Self {
        Self::Number(id)
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(id) => id.fmt(f),
            Self::Text(id) => id.fmt(f),
        }
    }
}

/// The first line a client sends: `{"hello": {…}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Greeting {
    /// The client's token, protocol and name.
    pub hello: Hello,
}

impl Greeting {
    /// The line that opens a conversation with `hello`.
    #[must_use]
    pub const fn new(hello: Hello) -> Self {
        Self { hello }
    }
}

/// One request: `{"id": …, "method": "group.verb", "params": …}`. Absent `params` read as
/// `P::default()`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
// The schema shows absent params read as `P::default()`.
#[cfg_attr(
    feature = "schemars",
    derive(schemars::JsonSchema),
    schemars(bound = "P: schemars::JsonSchema + Default + serde::Serialize")
)]
#[non_exhaustive]
pub struct Request<P> {
    /// Echoed in the answer.
    pub id: RequestId,
    /// The method's wire name.
    pub method: String,
    /// The method's parameters.
    #[cfg_attr(feature = "serde", serde(default))]
    pub params: P,
}

impl<P> Request<P> {
    /// Request `id`: `method` with `params`.
    #[must_use]
    pub fn new(id: impl Into<RequestId>, method: impl Into<String>, params: P) -> Self {
        Self {
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

/// Every line an app sends, generic over a result's representation `R`.
///
/// Read by its keys (see the [module docs](self)), not by trying each shape in turn: a result
/// whose representation would also read an error, or nothing at all, is still an answer only
/// when the line names `result`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize), serde(untagged))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum ServerLine<R> {
    /// The answer to the client's hello: `{"hello": {…}}`.
    Hello {
        /// The protocol the app speaks on this connection, the app and what it can do.
        hello: Box<HelloReply>,
    },
    /// An event a subscription selected: `{"event": "frame", "params": {…}}`.
    Event(Event),
    /// A request that failed, or a refusal that answers none: `{"id": …, "error": {…}}`.
    Failed {
        /// The request it answers; absent for a line that answers none, such as a `version`
        /// refusal of the hello or a line that was not a request.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        id: Option<RequestId>,
        /// What went wrong.
        error: Box<ErrorObject>,
    },
    /// A request's result: `{"id": …, "result": …}`.
    Answered {
        /// The request it answers.
        id: RequestId,
        /// The method's output.
        result: R,
    },
}

impl<R> ServerLine<R> {
    /// The answer to the hello.
    #[must_use]
    pub fn hello(reply: HelloReply) -> Self {
        Self::Hello {
            hello: Box::new(reply),
        }
    }

    /// The answer to request `id`: its result, or the error it failed with.
    #[must_use]
    pub fn answer(id: RequestId, outcome: Result<R, ErrorObject>) -> Self {
        match outcome {
            Ok(result) => Self::Answered { id, result },
            Err(error) => Self::Failed {
                id: Some(id),
                error: Box::new(error),
            },
        }
    }

    /// A refusal that answers no request.
    #[must_use]
    pub fn refusal(error: ErrorObject) -> Self {
        Self::Failed {
            id: None,
            error: Box::new(error),
        }
    }

    /// The request this line answers, if it answers one.
    #[must_use]
    pub fn id(&self) -> Option<&RequestId> {
        match self {
            Self::Failed { id, .. } => id.as_ref(),
            Self::Answered { id, .. } => Some(id),
            Self::Hello { .. } | Self::Event(_) => None,
        }
    }
}

#[cfg(feature = "serde")]
mod read {
    //! Reading a [`ServerLine`] by its keys.

    use std::fmt;
    use std::marker::PhantomData;

    use serde::Deserialize as _;
    use serde::de::{
        self, DeserializeSeed, Deserializer, IgnoredAny, IntoDeserializer, MapAccess, Visitor,
    };

    use super::{RequestId, ServerLine};
    use crate::error::ErrorObject;
    use crate::events::Event;
    use crate::session::HelloReply;

    /// The keys a server line can carry; any other is ignored.
    #[derive(serde::Deserialize)]
    #[serde(field_identifier, rename_all = "snake_case")]
    enum Key {
        Hello,
        Id,
        Error,
        Result,
        Event,
        Params,
        #[serde(other)]
        Other,
    }

    impl<'de, R: serde::Deserialize<'de>> serde::Deserialize<'de> for ServerLine<R> {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            deserializer.deserialize_map(LineVisitor(PhantomData))
        }
    }

    struct LineVisitor<R>(PhantomData<R>);

    impl<'de, R: serde::Deserialize<'de>> Visitor<'de> for LineVisitor<R> {
        type Value = ServerLine<R>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a server line: an object naming `hello`, `event`, `error` or `result`")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut hello: Option<HelloReply> = None;
            let mut id: Option<Option<RequestId>> = None;
            let mut error: Option<ErrorObject> = None;
            let mut result: Option<R> = None;
            let mut kind: Option<String> = None;
            let mut event: Option<Event> = None;
            while let Some(key) = map.next_key::<Key>()? {
                match key {
                    Key::Hello => once(&mut hello, "hello", map.next_value()?)?,
                    Key::Id => once(&mut id, "id", map.next_value()?)?,
                    Key::Error => once(&mut error, "error", map.next_value()?)?,
                    Key::Result => once(&mut result, "result", map.next_value()?)?,
                    Key::Event => once(&mut kind, "event", map.next_value()?)?,
                    Key::Params => {
                        let Some(kind) = kind.as_deref() else {
                            return Err(de::Error::custom(
                                "an event line names its `event` before its `params`",
                            ));
                        };
                        let read = map.next_value_seed(EventSeed { kind })?;
                        once(&mut event, "params", read)?;
                    }
                    Key::Other => {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
            }
            let named = [
                hello.is_some(),
                kind.is_some(),
                error.is_some(),
                result.is_some(),
            ]
            .into_iter()
            .filter(|named| *named)
            .count();
            if named > 1 {
                return Err(de::Error::custom(
                    "a server line names one of `hello`, `event`, `error` and `result`; this one \
                     names several",
                ));
            }
            if let Some(hello) = hello {
                return Ok(ServerLine::hello(hello));
            }
            if kind.is_some() {
                // Every event carries params.
                return event
                    .map(ServerLine::Event)
                    .ok_or_else(|| de::Error::missing_field("params"));
            }
            if let Some(error) = error {
                return Ok(ServerLine::Failed {
                    id: id.flatten(),
                    error: Box::new(error),
                });
            }
            if let Some(result) = result {
                let id = id.flatten().ok_or_else(|| de::Error::missing_field("id"))?;
                return Ok(ServerLine::Answered { id, result });
            }
            Err(de::Error::custom(
                "a server line names one of `hello`, `event`, `error` and `result`; this one \
                 names none",
            ))
        }
    }

    fn once<T, E: de::Error>(slot: &mut Option<T>, key: &'static str, value: T) -> Result<(), E> {
        if slot.is_some() {
            return Err(E::duplicate_field(key));
        }
        *slot = Some(value);
        Ok(())
    }

    /// Reads an event's `params` once its kind is known, through [`Event`]'s own reader.
    struct EventSeed<'a> {
        kind: &'a str,
    }

    impl<'de> DeserializeSeed<'de> for EventSeed<'_> {
        type Value = Event;

        fn deserialize<D: Deserializer<'de>>(self, params: D) -> Result<Event, D::Error> {
            Event::deserialize(Tagged {
                kind: self.kind,
                params,
            })
        }
    }

    /// `{"event": kind, "params": …}` rebuilt for [`Event`]'s reader, the params read straight
    /// from the line's own deserializer.
    struct Tagged<'a, P> {
        kind: &'a str,
        params: P,
    }

    impl<'de, P: Deserializer<'de>> Deserializer<'de> for Tagged<'_, P> {
        type Error = P::Error;

        fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            visitor.visit_map(TaggedEntries {
                kind: self.kind,
                params: Some(self.params),
                next: Entry::Kind,
            })
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes
            byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct map struct
            enum identifier ignored_any
        }
    }

    /// Which entry of the rebuilt map comes next.
    enum Entry {
        Kind,
        KindValue,
        Params,
        ParamsValue,
        Done,
    }

    struct TaggedEntries<'a, P> {
        kind: &'a str,
        params: Option<P>,
        next: Entry,
    }

    impl<'de, P: Deserializer<'de>> MapAccess<'de> for TaggedEntries<'_, P> {
        type Error = P::Error;

        fn next_key_seed<K: DeserializeSeed<'de>>(
            &mut self,
            seed: K,
        ) -> Result<Option<K::Value>, Self::Error> {
            let (key, next) = match self.next {
                Entry::Kind => ("event", Entry::KindValue),
                Entry::Params => ("params", Entry::ParamsValue),
                Entry::KindValue | Entry::ParamsValue | Entry::Done => return Ok(None),
            };
            self.next = next;
            seed.deserialize(key.into_deserializer()).map(Some)
        }

        fn next_value_seed<V: DeserializeSeed<'de>>(
            &mut self,
            seed: V,
        ) -> Result<V::Value, Self::Error> {
            match self.next {
                Entry::KindValue => {
                    self.next = Entry::Params;
                    seed.deserialize(self.kind.into_deserializer())
                }
                Entry::ParamsValue => {
                    self.next = Entry::Done;
                    let params = self
                        .params
                        .take()
                        .ok_or_else(|| de::Error::custom("an event's params are read once"))?;
                    seed.deserialize(params)
                }
                Entry::Kind | Entry::Params | Entry::Done => {
                    Err(de::Error::custom("a value is read after its key"))
                }
            }
        }
    }
}
