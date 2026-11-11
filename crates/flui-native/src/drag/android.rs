//! Android drag and drop through `FluiView` (ADR-0211, ADR-0247).
//!
//! [`AndroidTransfer`] is the platform's one [`DragTransport`]. A drag
//! over the view mints an offer from its `ClipDescription` when it enters,
//! answers the view at once on the Java main thread, and queues the
//! `DragDropEvent`s for the owner. The drop is decided from the feedback the
//! target cached. The view holds the drop's `ClipData`, and its URI
//! permissions, under the offer's key before it asks, so nothing of the clip
//! crosses JNI at the drop: a request reads one representation, under its
//! limits, from the held clip. The clip's text, HTML and URI list are in this
//! process and are read on the requesting thread; a URI's bytes come from
//! another app's content provider and are read on a thread of their own, at
//! most `drops::MAX_READS` at once, more waiting their turn. A
//! read whose offer retires answers [`TransferError::SourceGone`] at once and
//! keeps its place until its thread ends; what it reads late is discarded.
//! Concluding the drop retires the offer and lets go of its clip and
//! permissions alone; a refused drop lets go of them at once, and so does
//! the view when it leaves the window. No completer is completed with the
//! drops locked.
//!
//! [`AndroidDragSource`] starts a system drag (`startDragAndDrop`, API 24)
//! when the runtime escalates one leaving the window. Android takes the whole
//! clip at the start, so every representation is served then; the view runs
//! the drag and reports its end, which reaches the request's end hook on the
//! owner thread.
//!
//! What a clip offers, how a read answers and what a FLUI drag carries is
//! decided in [`clip`], and the offers, drops, reads and their conclusions in
//! [`drops`], with no JNI call, so every host compiles and checks them; the
//! JNI side around them is Android's alone.

/// The decidable half of the backend (ADR-0247, ADR-0211): how a
/// `DragEvent`'s clip maps to an offer, how a read of the held clip answers a
/// request, what a FLUI drag puts in its `ClipData`, and how the source learns
/// the end. Plain values, so every host checks them; the JNI side around them
/// moves them across.
///
/// Android delivers a drag to a view through `View.onDragEvent` on the Java
/// main thread, which needs every answer at once: the view keeps receiving
/// the drag only if it accepts `ACTION_DRAG_STARTED`, and the drop's result is
/// the return of `ACTION_DROP`. FLUI's target answers on its owner thread, so
/// the drop is decided from the feedback the target cached for the offer, as
/// the Win32 and web targets decide theirs. Until the drop the clip's data is
/// out of reach and only its `ClipDescription` is known, so an offer is minted
/// from the description's media types; after it, the view holds the clip and
/// each request reads one representation of it.
#[forbid(unsafe_code)]
mod clip {
    use std::sync::Arc;

    use flui_platform::data_transfer::{
        Mime, RepresentationDescriptor, TransferActions, TransferError, TransferFormat,
        TransferLimits, TransferPayload,
    };
    use flui_platform::drag::{IDENTITY_ESSENCE, OsDragEnd};

    use crate::drag::offers::{PathStyle, TransferUri};

    /// `View.DRAG_FLAG_GLOBAL`: the drag may leave the source's window.
    pub(super) const DRAG_FLAG_GLOBAL: i32 = 1 << 8;
    /// `View.DRAG_FLAG_GLOBAL_URI_READ`: a target may read the drag's URIs.
    pub(super) const DRAG_FLAG_GLOBAL_URI_READ: i32 = 1;

    /// `ClipDescription.MIMETYPE_TEXT_INTENT`, which FLUI cannot redeem.
    pub(super) const MIMETYPE_TEXT_INTENT: &str = "text/vnd.android.intent";

    /// Where one announced representation is read from once the drop is
    /// taken.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum ClipSource {
        /// The items' text, joined (`FluiView.readDropText`).
        Text,
        /// The first item's HTML (`FluiView.readDropText`).
        Html,
        /// Every item's URI (`FluiView.readDropUris`).
        UriList,
        /// The bytes behind the first URI (`FluiView.readDropBytes`), from
        /// another app's content provider.
        UriBytes,
        /// FLUI's session identity: its media type says everything.
        Identity,
    }

    /// The representations a drag of `mime_types` offers, in the description's
    /// order: text, HTML and a URI list as themselves, FLUI's identity with its
    /// parameters (declared length zero), and any other media type as the bytes
    /// of the clip's URI. Intents, duplicates and names that are not media types
    /// are not announced.
    #[must_use]
    pub(super) fn offered<'a>(
        mime_types: impl IntoIterator<Item = &'a str>,
    ) -> Vec<(ClipSource, RepresentationDescriptor)> {
        let mut offered: Vec<(ClipSource, RepresentationDescriptor)> = Vec::new();
        for mime in mime_types {
            if mime == MIMETYPE_TEXT_INTENT {
                continue;
            }
            let Some(format) = crate::drag::offers::format_of(mime) else {
                continue;
            };
            if offered.iter().any(|(_, known)| known.format == format) {
                continue;
            }
            let identity = format.is_drag_identity();
            let source = match &format {
                TransferFormat::Text => ClipSource::Text,
                TransferFormat::Html => ClipSource::Html,
                TransferFormat::UriList => ClipSource::UriList,
                _ if identity => ClipSource::Identity,
                _ => ClipSource::UriBytes,
            };
            offered.push((
                source,
                RepresentationDescriptor {
                    format,
                    declared_len: identity.then_some(0),
                },
            ));
        }
        offered
    }

    /// What `FluiView` answered a read of the held clip with.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) enum ViewRead {
        /// `readDropText` or `readDropBytes`: the bytes, within the limit.
        Bytes(Vec<u8>),
        /// `readDropUris`: every item's URI, their list within the limit.
        Uris(Vec<String>),
        /// No answer: past the limit when `read` is, and otherwise nothing to
        /// read (the clip did not carry the representation, the drop was let
        /// go of, or the provider failed). `read` is the length the view
        /// counted, a lower bound of the size when it stopped past the limit.
        Stopped {
            /// What the view counted or read before it stopped.
            read: u64,
        },
    }

    /// The payload a request for `source` of `descriptor` resolves as, from
    /// what the view `read`, under `limits`: the session identity from its
    /// descriptor alone, text and HTML from their UTF-8, the URI list through
    /// [`TransferUri::list_payload`], and a URI's bytes as the format they
    /// were announced as. The delivered payload is held to `limits` again,
    /// whatever the view counted.
    ///
    /// # Errors
    ///
    /// [`TransferError::TooLarge`] past `limits`; [`TransferError::Decode`]
    /// for text that is not UTF-8; [`TransferError::SourceGone`] when the
    /// view had nothing to read, or answered with a read of another kind.
    pub(super) fn delivered(
        source: ClipSource,
        descriptor: &RepresentationDescriptor,
        read: ViewRead,
        limits: TransferLimits,
    ) -> Result<TransferPayload, TransferError> {
        let decode = || TransferError::Decode {
            format: descriptor.format.clone(),
        };
        let payload = match (source, read) {
            (ClipSource::Identity, _) => TransferPayload::Custom {
                mime: match &descriptor.format {
                    TransferFormat::Custom { mime } => mime.clone(),
                    _ => Mime::new(IDENTITY_ESSENCE),
                },
                bytes: Arc::from(Vec::new()),
            },
            (_, ViewRead::Stopped { read }) => {
                limits.enforce(read)?;
                return Err(TransferError::SourceGone);
            }
            (ClipSource::Text, ViewRead::Bytes(bytes)) => {
                TransferPayload::Text(String::from_utf8(bytes).map_err(|_| decode())?)
            }
            (ClipSource::Html, ViewRead::Bytes(bytes)) => {
                TransferPayload::Html(String::from_utf8(bytes).map_err(|_| decode())?)
            }
            (ClipSource::UriList, ViewRead::Uris(uris)) if !uris.is_empty() => {
                TransferUri::list_payload(uris.into_iter().map(TransferUri::Uri), PathStyle::Posix)
            }
            (ClipSource::UriBytes, ViewRead::Bytes(bytes)) => {
                crate::drag::offers::payload_of_bytes(&descriptor.format, bytes)
            }
            _ => return Err(TransferError::SourceGone),
        };
        limits.enforce(payload.byte_len())?;
        Ok(payload)
    }

    /// The actions a drag over FLUI permits. Android has no drop effects: a drag
    /// from another app is a copy. A drag this process started (its local state
    /// is set) permits what its source advertised.
    #[must_use]
    pub(super) fn incoming_allowed(own: Option<TransferActions>) -> TransferActions {
        own.unwrap_or(TransferActions::COPY)
    }

    /// What a FLUI drag puts in its `ClipData`.
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub(super) struct ClipParts {
        /// The description's media types, the session identity among them.
        pub(super) mime_types: Vec<String>,
        /// The text item's text.
        pub(super) text: Option<String>,
        /// The text item's HTML.
        pub(super) html: Option<String>,
        /// One item per URI.
        pub(super) uris: Vec<String>,
    }

    impl ClipParts {
        /// The `startDragAndDrop` flags: always global, since a sibling window
        /// of the runtime is another window too, and letting targets read the
        /// URIs when there are any.
        #[must_use]
        pub(super) fn flags(&self) -> i32 {
            if self.uris.is_empty() {
                DRAG_FLAG_GLOBAL
            } else {
                DRAG_FLAG_GLOBAL | DRAG_FLAG_GLOBAL_URI_READ
            }
        }
    }

    /// The clip of a drag offering `representations`, each served (or not) as
    /// `payloads` says, in the same order.
    ///
    /// Text and HTML share the first item; each URI is an item of its own. A
    /// `file:` URI or a path is left out: Android refuses to let one leave the
    /// process through a clip (`FileUriExposedException` from API 24). Bytes have
    /// no place in a clip without a content provider, so a representation served
    /// as bytes is left out too, except FLUI's identity, which travels as its
    /// media type alone.
    #[must_use]
    pub(super) fn clip_parts(
        representations: &[RepresentationDescriptor],
        payloads: &[Option<TransferPayload>],
    ) -> ClipParts {
        let mut parts = ClipParts::default();
        for (descriptor, payload) in representations.iter().zip(payloads) {
            let identity = descriptor.format.is_drag_identity();
            let carried = match payload {
                _ if identity => true,
                Some(TransferPayload::Text(text)) => {
                    parts.text = Some(text.clone());
                    true
                }
                Some(TransferPayload::Html(html)) => {
                    parts.html = Some(html.clone());
                    true
                }
                Some(payload) => match TransferUri::list_of(payload, PathStyle::Posix) {
                    Some(uris) => {
                        let before = parts.uris.len();
                        parts
                            .uris
                            .extend(uris.into_iter().filter_map(|uri| match uri {
                                TransferUri::Uri(uri) if !uri.starts_with("file:") => Some(uri),
                                _ => None,
                            }));
                        parts.uris.len() > before
                    }
                    None => false,
                },
                None => false,
            };
            if carried {
                parts.mime_types.push(mime_type(&descriptor.format));
            }
        }
        parts
    }

    fn mime_type(format: &TransferFormat) -> String {
        match format {
            TransferFormat::Text => "text/plain".to_owned(),
            TransferFormat::Html => "text/html".to_owned(),
            TransferFormat::UriList => "text/uri-list".to_owned(),
            TransferFormat::Image { mime } | TransferFormat::Custom { mime } => {
                mime.as_str().to_owned()
            }
        }
    }

    /// How a FLUI drag ended, from `ACTION_DRAG_ENDED`'s result: refused or
    /// cancelled when no target took it; the action FLUI's own target took when
    /// it landed in this process; otherwise copy when the drag allowed it, since
    /// another app's drop is a copy, else the one action it allowed first.
    #[must_use]
    pub(super) fn source_end(
        result: bool,
        local_drop: Option<TransferActions>,
        allowed: TransferActions,
    ) -> OsDragEnd {
        if !result {
            return OsDragEnd::Cancelled;
        }
        if let Some(action) = local_drop {
            return OsDragEnd::Dropped(action);
        }
        let action = [
            TransferActions::COPY,
            TransferActions::MOVE,
            TransferActions::LINK,
        ]
        .into_iter()
        .find(|action| allowed.contains(*action));
        action.map_or(OsDragEnd::Cancelled, OsDragEnd::Dropped)
    }

    #[cfg(test)]
    mod tests;
}

/// The drags over the view and the drops taken from them, as plain state:
/// each `DragEvent` the view reports becomes the [`DragDropEvent`]s it queues
/// and the answer it returns, a taken drop keeps where each representation is
/// read from until the target concludes it, and each read of a URI's bytes
/// waits for a place and is answered once. Every host checks it; the JNI side
/// around it reports the events, holds the clip in the view and runs the
/// reads.
///
/// A drop the target has not concluded keeps its offer, so drops overlap: a
/// new drag may enter, drop and conclude while an earlier drop is still being
/// read, and each conclusion retires its own offer alone.
#[cfg(any(target_os = "android", test))]
#[forbid(unsafe_code)]
mod drops {
    use std::collections::{HashMap, VecDeque};
    use std::num::NonZeroU64;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use flui_foundation::{DataTransferId, IdMint, geometry::Point, panic::FirstPanic};
    use flui_platform::DragDropEvent;
    use flui_platform::data_transfer::{
        DataTransferOffer, DropFeedback, RepresentationDescriptor, RepresentationIndex,
        TransferActions, TransferCompleter, TransferError, TransferLimits, TransferRequest,
    };
    use parking_lot::Mutex;

    use super::clip::{ClipSource, ViewRead, delivered, incoming_allowed, offered};
    use crate::drag::offers::OfferTable;

    /// `DragEvent.ACTION_DRAG_STARTED`.
    pub(super) const ACTION_DRAG_STARTED: i32 = 1;
    /// `DragEvent.ACTION_DRAG_LOCATION`.
    pub(super) const ACTION_DRAG_LOCATION: i32 = 2;
    /// `DragEvent.ACTION_DRAG_ENDED`.
    pub(super) const ACTION_DRAG_ENDED: i32 = 4;
    /// `DragEvent.ACTION_DRAG_ENTERED`.
    pub(super) const ACTION_DRAG_ENTERED: i32 = 5;
    /// `DragEvent.ACTION_DRAG_EXITED`.
    pub(super) const ACTION_DRAG_EXITED: i32 = 6;

    /// At most this many URIs of drops are read at once, each on a thread of
    /// its own; more wait their turn, as the X11 and Wayland targets' fetches
    /// do.
    pub(super) const MAX_READS: usize = 4;

    /// At most this many reads of URIs wait for a place; a request past them
    /// is refused [`TransferError::Busy`]. One whose request was dropped gives
    /// its entry back when the next is asked for.
    pub(super) const MAX_WAITING: usize = 64;

    /// How long a request for a URI's bytes may take from when it is
    /// accepted, its wait for a place included, as the X11 and Wayland
    /// targets give a fetch: past it the request is answered
    /// [`TransferError::TimedOut`] and a Java read it started is unblocked.
    pub(super) const READ_TIMEOUT: Duration = Duration::from_secs(5);

    /// How often the watch tries again to start a cancel worker that could
    /// not start ([`Drops::uncancel`]).
    pub(super) const CANCEL_RETRY: Duration = Duration::from_secs(1);

    /// The key `FluiView` holds a taken drop's clip and URI permissions under:
    /// the offer's whole identity, generation included, so a concluded offer
    /// never lets go of a later one's. Never zero, which the view reads as no
    /// offer.
    #[must_use]
    pub(super) fn java_key(id: DataTransferId) -> i64 {
        id.as_u64().cast_signed()
    }

    /// How a request for one representation of a taken drop goes on.
    #[derive(Debug)]
    pub(super) enum Requested {
        /// Read from the held clip on the requesting thread, which is in this
        /// process: its text, HTML or URI list, under the view's `key`.
        Clip {
            /// The drop's [`java_key`].
            key: i64,
            /// What the read is.
            source: ClipSource,
            /// The representation as announced.
            descriptor: RepresentationDescriptor,
        },
        /// Answered already, or waiting for a read of a URI's bytes.
        Request(TransferRequest),
    }

    /// A read of a URI's bytes to run on a thread of its own, its result
    /// handed back to [`Drops::finish_read`] under its `ticket`.
    #[derive(Debug)]
    pub(super) struct UriRead {
        /// The read's place among the [`MAX_READS`].
        pub(super) ticket: NonZeroU64,
        /// The drop's [`java_key`].
        pub(super) key: i64,
        /// The representation as announced.
        pub(super) descriptor: RepresentationDescriptor,
        /// The request's limits.
        pub(super) limits: TransferLimits,
    }

    /// What a change of the drops leaves to do once they are unlocked: no
    /// completer is completed, no thread started and no Java call made with
    /// them locked.
    #[derive(Debug, Default)]
    pub(super) struct After {
        /// Reads that found a place, to start.
        pub(super) start: Vec<UriRead>,
        /// Requests to answer [`TransferError::SourceGone`]: their offer
        /// retired, or their read could not start.
        pub(super) gone: Vec<TransferCompleter>,
        /// Requests to answer [`TransferError::TimedOut`]: their read passed
        /// [`READ_TIMEOUT`].
        pub(super) timed_out: Vec<TransferCompleter>,
        /// Whether the cancel worker must start, which unblocks the Java reads
        /// queued for it ([`Drops::next_cancel`]) off the owner thread and the
        /// watch.
        pub(super) cancel_worker: bool,
        /// Whether a watch must start, which ends the reads that pass their
        /// deadline ([`Drops::expire`]).
        pub(super) watch: bool,
    }

    impl After {
        /// Whether nothing is left to do.
        #[must_use]
        pub(super) fn is_empty(&self) -> bool {
            self.start.is_empty()
                && self.gone.is_empty()
                && self.timed_out.is_empty()
                && !self.cancel_worker
                && !self.watch
        }
    }

    /// A read of a URI's bytes waiting for a place.
    #[derive(Debug)]
    struct Wanted {
        id: DataTransferId,
        descriptor: RepresentationDescriptor,
        limits: TransferLimits,
        completer: TransferCompleter,
        /// When the request times out, set when it is accepted: a read that
        /// takes a place keeps it, so it bounds the whole wait.
        deadline: Instant,
    }

    /// A read of a URI's bytes holding a place until its thread ends; its
    /// completer is taken when the offer retires or the deadline passes
    /// first.
    #[derive(Debug)]
    struct Reading {
        id: DataTransferId,
        completer: Option<TransferCompleter>,
        /// When the read times out.
        deadline: Instant,
        /// Whether its thread entered the Java read ([`Drops::begin_read`]),
        /// so a cancellation must reach it.
        entered: bool,
        /// Whether its Java read was asked to stop.
        cancelled: bool,
    }

    impl Reading {
        /// Stop the read: `true` when its thread is inside the Java read, which
        /// must be unblocked; one that has not entered it finds nothing
        /// wanted.
        fn stop(&mut self) -> bool {
            let unblock = self.entered && !self.cancelled;
            self.cancelled = true;
            unblock
        }
    }

    #[derive(Default)]
    pub(super) struct Drops {
        table: OfferTable,
        /// The answer each live offer's target last cached.
        feedback: HashMap<DataTransferId, DropFeedback>,
        /// Where each representation of a hovering offer is read from once
        /// it drops.
        sources: HashMap<DataTransferId, Vec<ClipSource>>,
        /// Where each representation of a taken drop is read from.
        dropped: HashMap<DataTransferId, Vec<ClipSource>>,
        /// The offer over the view and the actions its drag permits.
        hover: Option<(DataTransferId, TransferActions)>,
        /// The media types of the drag in progress, from `ACTION_DRAG_STARTED`.
        description: Vec<String>,
        /// What FLUI's own running drag advertised.
        own: Option<TransferActions>,
        /// The action FLUI's target took when FLUI's own drag dropped on it.
        local_drop: Option<TransferActions>,
        /// The tickets of the reads of URIs' bytes.
        tickets: IdMint<NonZeroU64>,
        /// The reads holding a place, at most [`MAX_READS`].
        reading: HashMap<NonZeroU64, Reading>,
        /// The reads waiting for a place, in the order they were asked for, at
        /// most [`MAX_WAITING`].
        waiting: VecDeque<Wanted>,
        /// Reads whose Java read the cancel worker is to unblock, at most one
        /// per read holding a place.
        cancels: VecDeque<NonZeroU64>,
        /// Whether the cancel worker runs ([`Self::next_cancel`]).
        cancelling: bool,
        /// Whether a watch runs ([`Self::expire`]).
        watching: bool,
    }

    impl Drops {
        fn mint(&mut self, mime_types: &[String]) -> DataTransferOffer {
            let (sources, representations): (Vec<ClipSource>, Vec<RepresentationDescriptor>) =
                offered(mime_types.iter().map(String::as_str))
                    .into_iter()
                    .unzip();
            let representations: Arc<[RepresentationDescriptor]> = Arc::from(representations);
            let id = self.table.mint(Arc::clone(&representations));
            self.feedback.insert(id, DropFeedback::default());
            self.sources.insert(id, sources);
            DataTransferOffer::new(id, representations)
        }

        /// Retire `id`. Only a taken drop has reads, and only
        /// [`Self::conclude`] retires one, so the reads are its to answer.
        fn close(&mut self, id: DataTransferId) {
            let _ = self.table.retire(id);
            self.feedback.remove(&id);
            self.sources.remove(&id);
            self.dropped.remove(&id);
        }

        /// The hovering offer leaves the view, if one is over it.
        fn leave(&mut self, events: &mut Vec<DragDropEvent>) {
            if let Some((id, _)) = self.hover.take() {
                events.push(DragDropEvent::Exited { id });
                self.close(id);
            }
        }

        /// One `DragEvent` other than the drop: what the view answers, with
        /// the events it queues for the owner. `position` is in logical
        /// pixels; `local` is whether this activity started the drag.
        pub(super) fn drag(
            &mut self,
            action: i32,
            position: (f64, f64),
            mime_types: &[String],
            local: bool,
            events: &mut Vec<DragDropEvent>,
        ) -> bool {
            match action {
                ACTION_DRAG_STARTED => {
                    self.description = mime_types.to_vec();
                }
                ACTION_DRAG_ENTERED => {
                    self.leave(events);
                    let types = if mime_types.is_empty() {
                        self.description.clone()
                    } else {
                        mime_types.to_vec()
                    };
                    let allowed = incoming_allowed(self.own.filter(|_| local));
                    let offer = self.mint(&types);
                    self.hover = Some((offer.id(), allowed));
                    events.push(DragDropEvent::Entered {
                        offer,
                        allowed,
                        position: None,
                    });
                }
                ACTION_DRAG_LOCATION => {
                    if let Some((id, allowed)) = self.hover {
                        events.push(DragDropEvent::Moved {
                            id,
                            allowed,
                            position: Point::new(position.0, position.1),
                        });
                    }
                }
                ACTION_DRAG_EXITED => self.leave(events),
                ACTION_DRAG_ENDED => {
                    self.leave(events);
                    self.description.clear();
                }
                _ => return false,
            }
            true
        }

        /// The [`java_key`] of the hovering offer, under which the view holds
        /// the clip before it asks for the drop; 0 when none hovers.
        #[must_use]
        pub(super) fn hover_key(&self) -> i64 {
            self.hover.map_or(0, |(id, _)| java_key(id))
        }

        /// `ACTION_DROP` of the clip the view holds under `key`: the event the
        /// drop queues and whether it is taken, from the target's cached
        /// feedback. `None` when no offer hovers or `key` names another one,
        /// which changes nothing. No item of the clip is read here: a taken
        /// drop keeps where each representation is read from until
        /// [`Self::conclude`].
        pub(super) fn take_drop(
            &mut self,
            key: i64,
            position: (f64, f64),
            local: bool,
        ) -> Option<(DragDropEvent, bool)> {
            let (id, allowed) = self.hover.filter(|(id, _)| java_key(*id) == key)?;
            self.hover = None;
            let feedback = self.feedback.get(&id).copied().unwrap_or_default();
            let Some(action) = feedback.resolve(allowed) else {
                self.close(id);
                return Some((DragDropEvent::Exited { id }, false));
            };
            let sources = self.sources.remove(&id).unwrap_or_default();
            self.dropped.insert(id, sources);
            if local && self.own.is_some() {
                self.local_drop = Some(action);
            }
            Some((
                DragDropEvent::Dropped {
                    id,
                    action,
                    position: Some(Point::new(position.0, position.1)),
                },
                true,
            ))
        }

        /// A request for representation `index` of the dropped offer `id`
        /// under `limits`, at `now`: read from the clip at once, answered from the
        /// descriptor alone (the session identity), or queued as a read of a
        /// URI's bytes, which `after` starts when it finds a place.
        ///
        /// An error is answered at once: [`TransferError::SourceGone`] before
        /// the drop, while the clip's data is out of reach;
        /// [`TransferError::StaleOffer`] for a retired offer;
        /// [`TransferError::UnknownRepresentation`] past the offer's.
        pub(super) fn request(
            &mut self,
            id: DataTransferId,
            index: RepresentationIndex,
            limits: TransferLimits,
            now: Instant,
            after: &mut After,
        ) -> Requested {
            let (source, descriptor) = match self.source(id, index) {
                Ok(found) => found,
                Err(error) => return Requested::Request(TransferRequest::ready(Err(error))),
            };
            match source {
                ClipSource::Identity => Requested::Request(TransferRequest::ready(delivered(
                    source,
                    &descriptor,
                    ViewRead::Stopped { read: 0 },
                    limits,
                ))),
                ClipSource::UriBytes => {
                    // A request dropped while it waits gives its entry back
                    // now, even when every place is held by a read that
                    // blocks.
                    let (gone, kept): (VecDeque<Wanted>, VecDeque<Wanted>) =
                        std::mem::take(&mut self.waiting)
                            .into_iter()
                            .partition(|wanted| wanted.completer.is_cancelled());
                    self.waiting = kept;
                    after
                        .gone
                        .extend(gone.into_iter().map(|wanted| wanted.completer));
                    if self.waiting.len() >= MAX_WAITING {
                        return Requested::Request(TransferRequest::ready(Err(
                            TransferError::Busy,
                        )));
                    }
                    let (request, completer) = TransferRequest::pending();
                    self.waiting.push_back(Wanted {
                        id,
                        descriptor,
                        limits,
                        completer,
                        deadline: now + READ_TIMEOUT,
                    });
                    // The watch times it while it waits too.
                    self.ensure_watch(after);
                    self.start_waiting(now, after);
                    // Cancellations a worker could not start for try again.
                    self.queue_cancels(Vec::new(), after);
                    Requested::Request(request)
                }
                ClipSource::Text | ClipSource::Html | ClipSource::UriList => Requested::Clip {
                    key: java_key(id),
                    source,
                    descriptor,
                },
            }
        }

        fn source(
            &self,
            id: DataTransferId,
            index: RepresentationIndex,
        ) -> Result<(ClipSource, RepresentationDescriptor), TransferError> {
            let Some(sources) = self.dropped.get(&id) else {
                return Err(if self.sources.contains_key(&id) {
                    TransferError::SourceGone
                } else {
                    TransferError::StaleOffer(id)
                });
            };
            let at = usize::from(index.0);
            let unknown = || TransferError::UnknownRepresentation { id, index };
            let source = *sources.get(at).ok_or_else(unknown)?;
            let descriptor = self
                .table
                .get(id)
                .and_then(|representations| representations.get(at))
                .cloned()
                .ok_or_else(unknown)?;
            Ok((source, descriptor))
        }

        /// A watch starts through `after` when none runs.
        fn ensure_watch(&mut self, after: &mut After) {
            if !self.watching {
                self.watching = true;
                after.watch = true;
            }
        }

        /// Give the waiting reads the places the running ones leave, in
        /// order, each keeping the deadline its request was accepted with;
        /// one whose request was dropped meanwhile is let go of, and one past
        /// its deadline at `now` times out.
        fn start_waiting(&mut self, now: Instant, after: &mut After) {
            while self.reading.len() < MAX_READS {
                let Some(wanted) = self.waiting.pop_front() else {
                    return;
                };
                if wanted.completer.is_cancelled() {
                    after.gone.push(wanted.completer);
                    continue;
                }
                if wanted.deadline <= now {
                    after.timed_out.push(wanted.completer);
                    continue;
                }
                let Ok(ticket) = self.tickets.next() else {
                    after.gone.push(wanted.completer);
                    continue;
                };
                self.reading.insert(
                    ticket,
                    Reading {
                        id: wanted.id,
                        completer: Some(wanted.completer),
                        deadline: wanted.deadline,
                        entered: false,
                        cancelled: false,
                    },
                );
                after.start.push(UriRead {
                    ticket,
                    key: java_key(wanted.id),
                    descriptor: wanted.descriptor,
                    limits: wanted.limits,
                });
                self.ensure_watch(after);
            }
        }

        /// The thread of the read under `ticket` is about to enter the Java
        /// read: `true` when its request still waits for it (its offer is
        /// live, its deadline not passed, its request not dropped), from when
        /// a cancellation is sent to the view; `false` when it should not read.
        #[must_use]
        pub(super) fn begin_read(&mut self, ticket: NonZeroU64) -> bool {
            let Some(reading) = self.reading.get_mut(&ticket) else {
                return false;
            };
            let wanted = !reading.cancelled
                && reading
                    .completer
                    .as_ref()
                    .is_some_and(|completer| !completer.is_cancelled());
            reading.entered = wanted;
            wanted
        }

        /// The read under `ticket` ended, its thread out of the Java read: its
        /// place goes to the next waiting read, and its completer is returned
        /// to answer, `None` when its offer retired or its deadline passed
        /// first and it was answered already.
        pub(super) fn finish_read(
            &mut self,
            ticket: NonZeroU64,
            now: Instant,
            after: &mut After,
        ) -> Option<TransferCompleter> {
            let reading = self.reading.remove(&ticket);
            // Its thread is out of the Java read: nothing is left to unblock.
            self.cancels.retain(|queued| *queued != ticket);
            self.start_waiting(now, after);
            reading.and_then(|reading| reading.completer)
        }

        /// The watch's turn at `now`: each read past its deadline is answered
        /// [`TransferError::TimedOut`] and its Java read unblocked, keeping
        /// its place until its thread ends, and each request still waiting
        /// for a place past its deadline is answered the same and let go of.
        /// Returns when to look again, the earliest deadline still to come,
        /// or [`CANCEL_RETRY`] from `now` while cancellations wait for a
        /// worker that could not start, which this turn starts again; `None`
        /// ends the watch, and the next request starts another.
        pub(super) fn expire(&mut self, now: Instant, after: &mut After) -> Option<Instant> {
            // Cancellations queued with no worker are left by one that could
            // not start ([`Self::uncancel`]): the watch stays to try again.
            let mut next =
                (!self.cancelling && !self.cancels.is_empty()).then(|| now + CANCEL_RETRY);
            let mut unblock = Vec::new();
            for (ticket, reading) in &mut self.reading {
                if reading.cancelled {
                    continue;
                }
                if reading.deadline <= now {
                    after.timed_out.extend(reading.completer.take());
                    if reading.stop() {
                        unblock.push(*ticket);
                    }
                } else {
                    next = Some(next.map_or(reading.deadline, |next| next.min(reading.deadline)));
                }
            }
            // A waiting request times out where it waits: no read has to end
            // for it, so reads that block for good hold up no deadline.
            let (late, kept): (VecDeque<Wanted>, VecDeque<Wanted>) =
                std::mem::take(&mut self.waiting)
                    .into_iter()
                    .partition(|wanted| wanted.deadline <= now);
            self.waiting = kept;
            after
                .timed_out
                .extend(late.into_iter().map(|wanted| wanted.completer));
            if let Some(first) = self.waiting.iter().map(|wanted| wanted.deadline).min() {
                next = Some(next.map_or(first, |next| next.min(first)));
            }
            self.watching = next.is_some();
            self.queue_cancels(unblock, after);
            next
        }

        /// Queue `tickets` for the cancel worker, starting it through `after`
        /// when none runs.
        fn queue_cancels(&mut self, tickets: Vec<NonZeroU64>, after: &mut After) {
            self.cancels.extend(tickets);
            if !self.cancelling && !self.cancels.is_empty() {
                self.cancelling = true;
                after.cancel_worker = true;
            }
        }

        /// The cancel worker's next read to unblock; `None` ends the worker,
        /// and the next read stopped inside its Java read starts another.
        pub(super) fn next_cancel(&mut self) -> Option<NonZeroU64> {
            let next = self.cancels.pop_front();
            self.cancelling = next.is_some();
            next
        }

        /// The cancel worker could not start: the watch tries again each
        /// [`CANCEL_RETRY`], a watch starting through `after` when none runs,
        /// and so does the next read stopped or asked for.
        pub(super) fn uncancel(&mut self, after: &mut After) {
            self.cancelling = false;
            if !self.watching {
                self.watching = true;
                after.watch = true;
            }
        }

        /// The watch could not start, so nothing would time the accepted
        /// requests: each waiting one and each running read is refused
        /// [`TransferError::SourceGone`] through `after`, a running read
        /// stopped and its thread unblocked when inside its Java read, and
        /// the next request tries for a watch again.
        pub(super) fn unwatched(&mut self, after: &mut After) {
            self.watching = false;
            after.gone.extend(
                std::mem::take(&mut self.waiting)
                    .into_iter()
                    .map(|wanted| wanted.completer),
            );
            let mut unblock = Vec::new();
            for (ticket, reading) in &mut self.reading {
                if reading.cancelled {
                    continue;
                }
                after.gone.extend(reading.completer.take());
                if reading.stop() {
                    unblock.push(*ticket);
                }
            }
            // Only reads it stopped ask for the worker: a worker and a watch
            // that both cannot start do not ask for each other in turn.
            if !unblock.is_empty() {
                self.queue_cancels(unblock, after);
            }
        }

        /// The target's answer for the live offer `id`, cached for the drop.
        pub(super) fn update_feedback(&mut self, id: DataTransferId, feedback: DropFeedback) {
            if let Some(cached) = self.feedback.get_mut(&id) {
                *cached = feedback;
            }
        }

        /// The target is done with the drop of `id`: its offer retires, its
        /// reads answer `SourceGone` through `after` (a running one is
        /// unblocked and keeps its place until its thread ends), and `true`
        /// when it was a taken drop, whose clip and URI permissions the view
        /// lets go of. A stale, hovering or repeated conclusion has nothing to
        /// let go of.
        pub(super) fn conclude(&mut self, id: DataTransferId, after: &mut After) -> bool {
            let taken = self.dropped.contains_key(&id);
            self.close(id);
            let mut unblock = Vec::new();
            for (ticket, reading) in &mut self.reading {
                if reading.id == id {
                    after.gone.extend(reading.completer.take());
                    if reading.stop() {
                        unblock.push(*ticket);
                    }
                }
            }
            let (gone, kept): (VecDeque<Wanted>, VecDeque<Wanted>) =
                std::mem::take(&mut self.waiting)
                    .into_iter()
                    .partition(|wanted| wanted.id == id);
            self.waiting = kept;
            after
                .gone
                .extend(gone.into_iter().map(|wanted| wanted.completer));
            self.queue_cancels(unblock, after);
            taken
        }

        /// FLUI's own drag started with `allowed`, or ended (`None`).
        pub(super) fn set_own(&mut self, allowed: Option<TransferActions>) {
            self.own = allowed;
            self.local_drop = None;
        }

        /// The action FLUI's target took when FLUI's own drag dropped on it.
        pub(super) fn take_local_drop(&mut self) -> Option<TransferActions> {
            self.local_drop.take()
        }
    }

    /// The watch over the running reads, on a thread of its own: at each
    /// deadline the reads past it time out ([`Drops::expire`]), `run` does
    /// what that left with the drops unlocked, and `sleep` waits for the next
    /// deadline read from `now`. It ends when no read is left to time, which
    /// clears the watch's flag under the lock. A panic in `run` (a waker's,
    /// a subscriber's) is held while the watch goes on, so no later deadline
    /// is missed, and raised once it has ended.
    pub(super) fn watch(
        drops: &Mutex<Drops>,
        now: impl Fn() -> Instant,
        sleep: impl Fn(Instant),
        mut run: impl FnMut(After),
    ) {
        let mut failure = FirstPanic::new();
        loop {
            let mut after = After::default();
            let next = drops.lock().expire(now(), &mut after);
            failure.contain(|| run(after));
            let Some(next) = next else {
                break;
            };
            sleep(next);
        }
        failure.resume();
    }

    /// The cancel worker, on a thread of its own: `cancel` unblocks each read
    /// queued for it ([`Drops::next_cancel`]), with the drops unlocked, so a
    /// cancellation that blocks in a content provider delays neither the
    /// owner thread nor the watch. It ends when the queue is empty, which
    /// clears its flag under the lock; a panic in `cancel` is held while the
    /// rest are unblocked and raised once it has ended.
    pub(super) fn cancel_all(drops: &Mutex<Drops>, mut cancel: impl FnMut(NonZeroU64)) {
        let mut failure = FirstPanic::new();
        loop {
            let next = drops.lock().next_cancel();
            let Some(ticket) = next else {
                break;
            };
            failure.contain(|| cancel(ticket));
        }
        failure.resume();
    }

    /// A thread [`settle`] asks to start.
    #[derive(Debug)]
    pub(super) enum Spawn {
        /// The cancel worker ([`cancel_all`]).
        CancelWorker,
        /// The watch ([`watch`]).
        Watch,
        /// A read of a URI's bytes, which answers through
        /// [`Drops::finish_read`].
        Read(UriRead),
    }

    /// Do what a change of `drops` left, with them unlocked: answer the
    /// requests whose offer retired or whose read timed out, then start the
    /// cancel worker, the watch and the reads that found a place through
    /// `spawn`, at `now`. A thread that cannot start leaves no accepted
    /// request unanswered: without the watch no request waits with no
    /// deadline, so every waiting request and running read is refused
    /// [`TransferError::SourceGone`] ([`Drops::unwatched`]) and none of the
    /// reads just placed is started; a
    /// read that cannot start is refused the same and gives its place on;
    /// the cancel worker that cannot start is tried again by the watch
    /// ([`Drops::uncancel`]). A waker's panic is raised once everything is
    /// done.
    pub(super) fn settle(
        drops: &Mutex<Drops>,
        mut after: After,
        now: impl Fn() -> Instant,
        mut spawn: impl FnMut(Spawn) -> std::io::Result<()>,
    ) {
        let mut failure = FirstPanic::new();
        while !after.is_empty() {
            for completer in std::mem::take(&mut after.gone) {
                failure.contain(|| completer.complete(Err(TransferError::SourceGone)));
            }
            for completer in std::mem::take(&mut after.timed_out) {
                failure.contain(|| completer.complete(Err(TransferError::TimedOut)));
            }
            if std::mem::take(&mut after.cancel_worker)
                && let Err(error) = spawn(Spawn::CancelWorker)
            {
                failure.contain(|| {
                    tracing::warn!(target: "flui_native::android", %error, "no thread to cancel dropped URI reads");
                });
                drops.lock().uncancel(&mut after);
            }
            let mut unwatched = false;
            if std::mem::take(&mut after.watch)
                && let Err(error) = spawn(Spawn::Watch)
            {
                failure.contain(|| {
                    tracing::warn!(target: "flui_native::android", %error, "no thread to time dropped URI reads");
                });
                drops.lock().unwatched(&mut after);
                unwatched = true;
            }
            for read in std::mem::take(&mut after.start) {
                let ticket = read.ticket;
                if !unwatched {
                    let Err(error) = spawn(Spawn::Read(read)) else {
                        continue;
                    };
                    failure.contain(|| {
                        tracing::warn!(target: "flui_native::android", %error, "no thread to read a dropped URI");
                    });
                }
                let completer = drops.lock().finish_read(ticket, now(), &mut after);
                after.gone.extend(completer);
            }
        }
        failure.resume();
    }
    #[cfg(test)]
    mod tests;
}

// The JNI side, on Android alone.

#[cfg(target_os = "android")]
use std::{
    num::NonZeroU64,
    sync::{Arc, Weak},
    time::Instant,
};

#[cfg(target_os = "android")]
use flui_foundation::{DataTransferId, panic::FirstPanic};
#[cfg(target_os = "android")]
use flui_platform::{
    Unsupported, UnsupportedReason,
    data_transfer::{
        DragTransport, DropFeedback, RepresentationIndex, TransferActions, TransferError,
        TransferLimits, TransferPayload, TransferRequest,
    },
    drag::{DragError, OsDragEndHook, OsDragRequest, OsDragStart, PlatformDragSource},
};
#[cfg(target_os = "android")]
use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JByteArray, JLongArray, JObject, JObjectArray, JString},
};
#[cfg(target_os = "android")]
use parking_lot::Mutex;

#[cfg(target_os = "android")]
use crate::{
    owner::android::java_view::{ActivityDragSource, JavaView, ViewEvent, ViewTransfer},
    owner::panic_boundary::contain_owner_callback,
    owner::slot::OwnerSlot,
    version::gate::Since,
    version::gate::android::{AndroidApi, DragSource},
};
#[cfg(target_os = "android")]
use clip::{ClipSource, ViewRead, clip_parts, delivered, source_end};
#[cfg(target_os = "android")]
use drops::{ACTION_DRAG_ENDED, After, Drops, Requested, Spawn, UriRead, java_key};

/// The size of the drag shadow, in logical pixels.
#[cfg(target_os = "android")]
const SHADOW: f64 = 48.0;

/// The platform's drag-and-drop transport (see the module doc).
#[cfg(target_os = "android")]
pub struct AndroidTransfer {
    view: Weak<JavaView>,
    /// Shared with the threads that read URIs' bytes.
    drops: Arc<Mutex<Drops>>,
}

#[cfg(target_os = "android")]
impl std::fmt::Debug for AndroidTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidTransfer").finish_non_exhaustive()
    }
}

#[cfg(target_os = "android")]
impl ViewTransfer for AndroidTransfer {
    /// One `DragEvent` other than the drop, on the Java main thread: what the
    /// view answers. `position` is in logical pixels; `local` is whether this
    /// activity started the drag (for `ACTION_DRAG_ENDED`, whether the view
    /// did).
    fn drag(
        &self,
        view: &JavaView,
        action: i32,
        position: (f64, f64),
        mime_types: &[String],
        local: bool,
        result: bool,
    ) -> bool {
        let mut events = Vec::new();
        let answer = self
            .drops
            .lock()
            .drag(action, position, mime_types, local, &mut events);
        for event in events {
            view.push_drag(event);
        }
        if action == ACTION_DRAG_ENDED && local {
            view.push_event(ViewEvent::DragEnded { result });
        }
        answer
    }

    fn hover_key(&self) -> i64 {
        self.drops.lock().hover_key()
    }

    /// `ACTION_DROP` of the clip the view holds under `key`: whether the drop
    /// is taken. Nothing of the clip crosses here; a request reads it until
    /// the target concludes the drop.
    fn take_drop(&self, view: &JavaView, key: i64, position: (f64, f64), local: bool) -> bool {
        let taken = self.drops.lock().take_drop(key, position, local);
        let Some((event, taken)) = taken else {
            return false;
        };
        view.push_drag(event);
        taken
    }

    fn set_own(&self, allowed: Option<TransferActions>) {
        self.drops.lock().set_own(allowed);
    }

    fn take_local_drop(&self) -> Option<TransferActions> {
        self.drops.lock().take_local_drop()
    }

    fn drag_source(&self, view: Arc<JavaView>, api: AndroidApi) -> Arc<dyn ActivityDragSource> {
        Arc::new(AndroidDragSource::new(view, api))
    }
}

#[cfg(target_os = "android")]
impl AndroidTransfer {
    /// The transport of `view`, which it reaches back weakly: the Android
    /// table's [`ViewTransfer`].
    pub(crate) fn for_view(view: Weak<JavaView>) -> Arc<dyn ViewTransfer> {
        Arc::new(Self {
            view,
            drops: Arc::default(),
        })
    }
}

#[cfg(target_os = "android")]
impl DragTransport for AndroidTransfer {
    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest {
        let mut after = After::default();
        let requested =
            self.drops
                .lock()
                .request(id, representation, limits, Instant::now(), &mut after);
        run(&self.drops, &self.view, after);
        match requested {
            Requested::Request(request) => request,
            Requested::Clip {
                key,
                source,
                descriptor,
            } => {
                let read = self
                    .view
                    .upgrade()
                    .map_or(ViewRead::Stopped { read: 0 }, |view| {
                        read_clip(&view, key, source, limits)
                    });
                TransferRequest::ready(delivered(source, &descriptor, read, limits))
            }
        }
    }

    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback) {
        self.drops.lock().update_feedback(id, feedback);
    }

    fn conclude_drop(&self, id: DataTransferId) {
        let mut after = After::default();
        let taken = self.drops.lock().conclude(id, &mut after);
        if taken && let Some(view) = self.view.upgrade() {
            let offer = java_key(id);
            view.post(move |env, view| {
                env.call_method(
                    view,
                    jni_str!("releaseDrop"),
                    jni_sig!("(J)V"),
                    &[JValue::Long(offer)],
                )
                .map(drop)
            });
        }
        run(&self.drops, &self.view, after);
    }
}

/// Do what a change of `drops` left, with them unlocked ([`drops::settle`]),
/// on the real clock and real threads.
#[cfg(target_os = "android")]
fn run(drops: &Arc<Mutex<Drops>>, view: &Weak<JavaView>, after: After) {
    drops::settle(drops, after, Instant::now, |job| spawn(drops, view, job));
}

/// Start the thread `job` names.
#[cfg(target_os = "android")]
fn spawn(drops: &Arc<Mutex<Drops>>, view: &Weak<JavaView>, job: Spawn) -> std::io::Result<()> {
    let (drops, view) = (Arc::clone(drops), Weak::clone(view));
    let thread = std::thread::Builder::new();
    match job {
        Spawn::CancelWorker => thread
            .name("flui-drop-cancel".to_owned())
            .spawn(move || drops::cancel_all(&drops, |ticket| cancel_read(&view, ticket))),
        Spawn::Watch => thread
            .name("flui-drop-watch".to_owned())
            .spawn(move || watch(&drops, &view)),
        Spawn::Read(read) => thread
            .name("flui-drop-read".to_owned())
            .spawn(move || read_on_thread(&drops, &view, read)),
    }
    .map(drop)
}

/// The watch over the running reads ([`drops::watch`]), on the real clock.
#[cfg(target_os = "android")]
fn watch(drops: &Arc<Mutex<Drops>>, view: &Weak<JavaView>) {
    drops::watch(
        drops,
        Instant::now,
        |next| std::thread::sleep(next.saturating_duration_since(Instant::now())),
        |after| run(drops, view, after),
    );
}

/// Unblock the Java read under `ticket` (`FluiView.cancelDropRead`): an open
/// in progress is cancelled and an open stream closed, which makes a blocked
/// read throw. On the cancel worker, which may block here.
#[cfg(target_os = "android")]
fn cancel_read(view: &Weak<JavaView>, ticket: NonZeroU64) {
    let Some(view) = view.upgrade() else {
        return;
    };
    let Some(target) = view.view() else {
        return;
    };
    let cancelled = view.attached(|env| {
        env.call_method(
            target.as_ref(),
            jni_str!("cancelDropRead"),
            jni_sig!("(J)V"),
            &[JValue::Long(ticket.get().cast_signed())],
        )
        .map(drop)
    });
    if let Err(error) = cancelled {
        tracing::warn!(target: "flui_native::android", %error, "a dropped URI read was not cancelled");
    }
}

/// Read the bytes of `read`'s URI, unless its request is gone, and answer it
/// unless its offer retired or its deadline passed first; then hand its place
/// on.
#[cfg(target_os = "android")]
fn read_on_thread(drops: &Arc<Mutex<Drops>>, view: &Weak<JavaView>, read: UriRead) {
    let mut failure = FirstPanic::new();
    let wanted = drops.lock().begin_read(read.ticket);
    let result = if wanted {
        failure
            .contain(|| {
                let bytes = view
                    .upgrade()
                    .map_or(ViewRead::Stopped { read: 0 }, |view| {
                        read_bytes(&view, read.key, read.ticket, read.limits)
                    });
                delivered(ClipSource::UriBytes, &read.descriptor, bytes, read.limits)
            })
            .unwrap_or(Err(TransferError::SourceGone))
    } else {
        Err(TransferError::SourceGone)
    };
    let mut after = After::default();
    let completer = drops
        .lock()
        .finish_read(read.ticket, Instant::now(), &mut after);
    if let Some(completer) = completer {
        failure.contain(|| completer.complete(result));
    }
    failure.contain(|| run(drops, view, after));
    failure.resume();
}

/// Representation `source` of the drop the view holds under `key`, read on
/// the calling thread: the clip's text, HTML or URIs, in this process.
#[cfg(target_os = "android")]
fn read_clip(view: &JavaView, key: i64, source: ClipSource, limits: TransferLimits) -> ViewRead {
    match source {
        ClipSource::Text | ClipSource::Html => {
            let html = source == ClipSource::Html;
            view_read(view, limits, |env, target, limit, count| {
                let text = env
                    .call_method(
                        target,
                        jni_str!("readDropText"),
                        jni_sig!("(JZJ[J)[B"),
                        &[
                            JValue::Long(key),
                            JValue::Bool(html),
                            JValue::Long(limit),
                            JValue::Object(count),
                        ],
                    )?
                    .l()?;
                byte_array(env, text)
            })
        }
        ClipSource::UriList => view_read(view, limits, |env, target, limit, count| {
            let uris = env
                .call_method(
                    target,
                    jni_str!("readDropUris"),
                    jni_sig!("(JJ[J)[Ljava/lang/String;"),
                    &[
                        JValue::Long(key),
                        JValue::Long(limit),
                        JValue::Object(count),
                    ],
                )?
                .l()?;
            if uris.is_null() {
                return Ok(None);
            }
            let uris = env.cast_local::<JObjectArray<'_>>(uris)?;
            let len = uris.len(env)?;
            let mut out = Vec::with_capacity(len);
            for index in 0..len {
                let uri = uris.get_element(env, index)?;
                let uri = env.cast_local::<JString<'_>>(uri)?;
                if !uri.is_null() {
                    out.push(uri.try_to_string(env)?);
                }
                // One local reference at a time, however long the list.
                env.delete_local_ref(uri);
            }
            Ok(Some(ViewRead::Uris(out)))
        }),
        ClipSource::UriBytes | ClipSource::Identity => ViewRead::Stopped { read: 0 },
    }
}

/// The bytes behind the first URI of the drop the view holds under `key`,
/// read through its content resolver on the calling thread as the read under
/// `ticket`, which `cancelDropRead` unblocks.
#[cfg(target_os = "android")]
fn read_bytes(view: &JavaView, key: i64, ticket: NonZeroU64, limits: TransferLimits) -> ViewRead {
    view_read(view, limits, |env, target, limit, count| {
        let bytes = env
            .call_method(
                target,
                jni_str!("readDropBytes"),
                jni_sig!("(JJJ[J)[B"),
                &[
                    JValue::Long(key),
                    JValue::Long(ticket.get().cast_signed()),
                    JValue::Long(limit),
                    JValue::Object(count),
                ],
            )?
            .l()?;
        byte_array(env, bytes)
    })
}

/// One of `FluiView`'s reads of a held drop: `read` calls it on the view with
/// the limit and the array it counts into, and answers what it returned, or
/// `None` when it stopped, which is answered with what it counted. A failed
/// call has nothing to read.
#[cfg(target_os = "android")]
fn view_read(
    view: &JavaView,
    limits: TransferLimits,
    read: impl FnOnce(
        &mut Env<'_>,
        &JObject<'_>,
        i64,
        &JLongArray<'_>,
    ) -> jni::errors::Result<Option<ViewRead>>,
) -> ViewRead {
    let Some(target) = view.view() else {
        return ViewRead::Stopped { read: 0 };
    };
    let limit = i64::try_from(limits.max_bytes).unwrap_or(i64::MAX);
    let answered = view.attached(|env| {
        let count = env.new_long_array(1)?;
        let value = read(env, target.as_ref(), limit, &count)?;
        let mut counted = [0_i64];
        count.get_region(env, 0, &mut counted)?;
        Ok((value, u64::try_from(counted[0]).unwrap_or(0)))
    });
    match answered {
        Ok((Some(value), _)) => value,
        Ok((None, read)) => ViewRead::Stopped { read },
        Err(_) => ViewRead::Stopped { read: 0 },
    }
}

#[cfg(target_os = "android")]
fn byte_array(env: &mut Env<'_>, bytes: JObject<'_>) -> jni::errors::Result<Option<ViewRead>> {
    if bytes.is_null() {
        return Ok(None);
    }
    let bytes = env.cast_local::<JByteArray<'_>>(bytes)?;
    Ok(Some(ViewRead::Bytes(env.convert_byte_array(&bytes)?)))
}

/// FLUI's drag running as a system drag.
#[cfg(target_os = "android")]
struct Running {
    allowed: TransferActions,
    on_end: OsDragEndHook,
}

/// A window's system drag source (see the module doc).
#[cfg(target_os = "android")]
pub(crate) struct AndroidDragSource {
    view: Arc<JavaView>,
    /// `View.startDragAndDrop`, which the device has from API 24.
    start_drag: Option<Since<DragSource>>,
    running: OwnerSlot<Running>,
}

#[cfg(target_os = "android")]
impl std::fmt::Debug for AndroidDragSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidDragSource").finish_non_exhaustive()
    }
}

#[cfg(target_os = "android")]
impl AndroidDragSource {
    fn new(view: Arc<JavaView>, api: AndroidApi) -> Self {
        Self {
            view,
            start_drag: api.since::<DragSource>(),
            running: OwnerSlot::new(),
        }
    }
}

#[cfg(target_os = "android")]
impl ActivityDragSource for AndroidDragSource {
    /// The system drag ended: `result` is whether a target took it. Owner
    /// thread.
    fn finish(&self, result: bool) {
        let Some(Running { allowed, on_end }) = self.running.take() else {
            return;
        };
        let local = self.view.transfer().take_local_drop();
        self.view.transfer().set_own(None);
        let end = source_end(result, local, allowed);
        contain_owner_callback(|| on_end(end));
    }

    /// The window closed: a running drag ends cancelled. Owner thread.
    fn retire(&self) {
        if let Some(Running { on_end, .. }) = self.running.take() {
            self.view.transfer().set_own(None);
            contain_owner_callback(|| on_end(flui_platform::drag::OsDragEnd::Cancelled));
        }
    }
}

/// Serve representation `index` through `provider`, containing a panic.
#[cfg(target_os = "android")]
fn serve(
    provider: &mut flui_platform::drag::OsDragProvider,
    index: u16,
) -> Option<TransferPayload> {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a drag provider's panic fails the transfer instead of crossing the JNI"
    )]
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        provider(RepresentationIndex(index)).ok()
    }))
    .unwrap_or_else(|payload| {
        flui_foundation::panic::discard_payload(payload);
        contain_owner_callback(|| {
            tracing::error!(target: "flui_native::android", "a drag's provider panicked");
        });
        None
    })
}

#[cfg(target_os = "android")]
fn string_array<'local>(
    env: &mut Env<'local>,
    values: &[String],
) -> jni::errors::Result<JObjectArray<'local>> {
    let array = env.new_object_array(
        i32::try_from(values.len()).unwrap_or(i32::MAX),
        jni_str!("java/lang/String"),
        JObject::null(),
    )?;
    for (index, value) in values.iter().enumerate() {
        let value = env.new_string(value)?;
        array.set_element(env, index, &value)?;
    }
    Ok(array)
}

#[cfg(target_os = "android")]
fn nullable<'local>(
    env: &mut Env<'local>,
    value: Option<&str>,
) -> jni::errors::Result<JString<'local>> {
    match value {
        Some(value) => env.new_string(value),
        None => Ok(JString::null()),
    }
}

#[cfg(target_os = "android")]
impl PlatformDragSource for AndroidDragSource {
    fn start_timing(&self) -> OsDragStart {
        crate::input::availability::DESKTOP_DRAG_START
    }

    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        // A device whose Android cannot start a drag never will; a view not
        // ready yet, or one running a drag, may later.
        let refused = if self.start_drag.is_none() {
            Some(UnsupportedReason::NotOnThisDevice)
        } else if !self.view.is_ready() || self.running.is_occupied() != Some(false) {
            Some(UnsupportedReason::Unavailable)
        } else {
            None
        };
        let OsDragRequest {
            key,
            representations,
            allowed,
            mut provider,
            on_end,
            ..
        } = request;
        if let Some(reason) = refused {
            contain_owner_callback(|| drop(provider));
            contain_owner_callback(|| drop(on_end));
            return Err(Unsupported::new(reason).into());
        }
        let payloads: Vec<Option<TransferPayload>> = (0..representations.len())
            .map(|index| serve(&mut provider, u16::try_from(index).unwrap_or(u16::MAX)))
            .collect();
        contain_owner_callback(|| drop(provider));
        let parts = clip_parts(&representations, &payloads);
        if let Err(Running { on_end, .. }) =
            self.running.replace(Running { allowed, on_end }).map(drop)
        {
            contain_owner_callback(|| drop(on_end));
            return Err(Unsupported::new(UnsupportedReason::Unavailable).into());
        }
        self.view.transfer().set_own(Some(allowed));
        let session = match key.representation().format {
            flui_platform::data_transfer::TransferFormat::Custom { mime } => {
                mime.as_str().to_owned()
            }
            _ => String::new(),
        };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a shadow of a few hundred pixels fits an i32"
        )]
        let shadow = (SHADOW * self.view.scale()).round() as i32;
        // A drag the view could not start ends as one no target took.
        let refused = |view: &JavaView| view.push_event(ViewEvent::DragEnded { result: false });
        self.view.post_or(move |env, view| {
            let label = env.new_string("FLUI")?;
            let mime_types = string_array(env, &parts.mime_types)?;
            let text = nullable(env, parts.text.as_deref())?;
            let html = nullable(env, parts.html.as_deref())?;
            let uris = string_array(env, &parts.uris)?;
            let session = env.new_string(&session)?;
            let started = env
                .call_method(
                    view,
                    jni_str!("startDrag"),
                    jni_sig!(
                        "(Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;IIILjava/lang/String;)Z"
                    ),
                    &[
                        JValue::Object(&label),
                        JValue::Object(&mime_types),
                        JValue::Object(&text),
                        JValue::Object(&html),
                        JValue::Object(&uris),
                        JValue::Int(parts.flags()),
                        JValue::Int(shadow),
                        JValue::Int(shadow),
                        JValue::Object(&session),
                    ],
                )?
                .z()?;
            tracing::info!(target: "flui_native::android", started, "startDragAndDrop answered");
            Ok(started)
        }, refused);
        Ok(())
    }
}
