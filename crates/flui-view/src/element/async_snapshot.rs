//! The state of an asynchronous computation observed by `FutureBuilder` or
//! `StreamBuilder`.
//!
//! A snapshot carries its connection state and at most one result. `Result`
//! makes data and errors mutually exclusive by construction; `Option` is needed
//! only where no result has ever existed. Builders pass the snapshot by
//! reference, so neither `T` nor `E` needs to be `Clone`.

/// The latest interaction with an asynchronous computation.
///
/// `None` and `Waiting` preserve the previous result while no current result is
/// available. `Active` always contains the stream event that made the stream
/// active. `Done` contains a future's result or a stream's last result; an empty
/// stream finishes as `Done(None)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AsyncSnapshot<T, E> {
    /// Not connected to an asynchronous computation.
    ///
    /// The optional result is initial data or the last result preserved across
    /// disconnection.
    None(Option<Result<T, E>>),

    /// Connected and waiting for the current computation's first result.
    ///
    /// A keyed resubscription preserves the previous result here.
    Waiting(Option<Result<T, E>>),

    /// A stream produced this event and remains connected.
    ///
    /// An error does not terminate the stream. A later event replaces this
    /// result, so data and errors from different events never coexist.
    Active(Result<T, E>),

    /// The future completed or the stream ended.
    ///
    /// A future always produces `Some(result)`. A stream preserves its last
    /// result, including an error, or produces `None` when it was empty.
    Done(Option<Result<T, E>>),
}

impl<T, E> AsyncSnapshot<T, E> {
    /// Move the latest result out of any connection state.
    pub(crate) fn into_last(self) -> Option<Result<T, E>> {
        match self {
            Self::None(last) | Self::Waiting(last) | Self::Done(last) => last,
            Self::Active(result) => Some(result),
        }
    }

    /// Name the connection state without formatting caller-owned payloads.
    pub(crate) const fn state_name(&self) -> &'static str {
        match self {
            Self::None(_) => "None",
            Self::Waiting(_) => "Waiting",
            Self::Active(_) => "Active",
            Self::Done(_) => "Done",
        }
    }
}
