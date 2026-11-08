//! Android drag and drop through `FluiView` (ADR-0211, ADR-0247).
//!
//! [`AndroidTransfer`] is the platform's one [`DragTransport`]. A drag
//! over the view mints an offer from its `ClipDescription` when it enters,
//! answers the view at once on the Java main thread, and queues the
//! `DragDropEvent`s for the owner. The drop is decided from the feedback the
//! target cached. Before that callback can wake the owner, the view saves the
//! `ClipData` and its URI grant under the offer's key; only the key and clip
//! descriptors cross JNI. A request reads one representation from that clip on
//! a thread of its own, with text, HTML, URI lists and URI bytes all bounded by
//! the request's UTF-8 byte limit. Concluding or refusing a drop releases its
//! clip and grant exactly once, and detaching the view releases every drop
//! still held.
//!
//! [`AndroidDragSource`] starts a system drag (`startDragAndDrop`, API 24)
//! when the runtime escalates one leaving the window. Android takes the whole
//! clip at the start, so every representation is served then; the view runs
//! the drag and reports its end, which reaches the request's end hook on the
//! owner thread.
//!
//! What a clip offers, reads and answers is decided in [`clip`], and the
//! offers, drops and their conclusions in [`drops`], with no JNI call, so every
//! host compiles and checks them; the JNI side around them is Android's alone.

/// The decidable half of the backend (ADR-0247, ADR-0211): how a
/// `DragEvent`'s clip maps to an offer, what a request reads, what a FLUI drag
/// puts in its `ClipData`, and how the source learns the end. Plain values,
/// so every host checks them; the JNI side around them moves them across.
///
/// Android delivers a drag to a view through `View.onDragEvent` on the Java
/// main thread, which needs every answer at once: the view keeps receiving
/// the drag only if it accepts `ACTION_DRAG_STARTED`, and the drop's result is
/// the return of `ACTION_DROP`. FLUI's target answers on its owner thread, so
/// the drop is decided from the feedback the target cached for the offer, as
/// the Win32 and web targets decide theirs. Until the drop the clip's data is
/// out of reach and only its `ClipDescription` is known, so an offer is minted
/// from the description's media types. Taking the drop reads no item; each
/// request later reads its representation from the saved clip.
#[forbid(unsafe_code)]
mod clip {
    use std::sync::Arc;

    use flui_platform::data_transfer::{
        Mime, RepresentationDescriptor, TransferActions, TransferError, TransferFormat,
        TransferLimits, TransferPayload,
    };
    use flui_platform::drag::{IDENTITY_ESSENCE, OsDragEnd};

    use crate::drag::offers::{PathStyle, TransferUri};
    use crate::transfer::intake::Intake;

    /// `View.DRAG_FLAG_GLOBAL`: the drag may leave the source's window.
    pub(super) const DRAG_FLAG_GLOBAL: i32 = 1 << 8;
    /// `View.DRAG_FLAG_GLOBAL_URI_READ`: a target may read the drag's URIs.
    pub(super) const DRAG_FLAG_GLOBAL_URI_READ: i32 = 1;

    /// `ClipDescription.MIMETYPE_TEXT_INTENT`, which FLUI cannot redeem.
    pub(super) const MIMETYPE_TEXT_INTENT: &str = "text/vnd.android.intent";

    /// Where one announced representation comes from when it is requested.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum ClipSource {
        /// The items' text, joined.
        Text,
        /// The first item's HTML.
        Html,
        /// Every item's URI.
        UriList,
        /// The bytes behind the first URI.
        UriBytes,
        /// FLUI's session identity: its media type says everything.
        Identity,
    }

    impl ClipSource {
        /// The representation kind in `FluiView.readDrop`'s protocol.
        #[cfg(target_os = "android")]
        pub(super) fn code(self) -> i32 {
            match self {
                Self::Text => 0,
                Self::Html => 1,
                Self::UriList => 2,
                Self::UriBytes => 3,
                Self::Identity => 4,
            }
        }

        /// Read this representation of a taken clip once, under `limits`.
        ///
        /// `read` is not called for the identity, which the descriptor already
        /// carries. For every other kind it reads the saved `ClipData` under the
        /// same limits; this method checks the delivered length with `limits`
        /// again before decoding it. `None` from the view is
        /// [`TransferError::SourceGone`], whether the representation was absent
        /// or its source failed.
        pub(super) fn request(
            self,
            descriptor: &RepresentationDescriptor,
            limits: TransferLimits,
            read: impl FnOnce(Self, TransferLimits) -> Result<Vec<u8>, TransferError>,
        ) -> Result<TransferPayload, TransferError> {
            let identity = || TransferPayload::Custom {
                mime: match &descriptor.format {
                    TransferFormat::Custom { mime } => mime.clone(),
                    _ => Mime::new(IDENTITY_ESSENCE),
                },
                bytes: Arc::from(Vec::new()),
            };
            if self == Self::Identity {
                return Ok(identity());
            }
            let bytes = read(self, limits)?;
            let actual = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            limits.check(actual)?;
            match self {
                Self::Text => String::from_utf8(bytes)
                    .map(TransferPayload::Text)
                    .map_err(|_| TransferError::Decode {
                        format: descriptor.format.clone(),
                    }),
                Self::Html => String::from_utf8(bytes)
                    .map(TransferPayload::Html)
                    .map_err(|_| TransferError::Decode {
                        format: descriptor.format.clone(),
                    }),
                Self::UriList => Ok(TransferPayload::Custom {
                    mime: Mime::new("text/uri-list"),
                    bytes: Arc::from(bytes),
                }),
                Self::UriBytes => Ok(crate::drag::offers::payload_of_bytes(
                    &descriptor.format,
                    bytes,
                )),
                Self::Identity => Ok(identity()),
            }
        }
    }

    /// The representations a drag of `mime_types` offers, in the description's
    /// order: text, HTML and a URI list as themselves, FLUI's identity with its
    /// parameters (declared length zero), and any other media type as the bytes
    /// of the clip's URI. The common intake policy bounds source entries, name
    /// lengths and duplicate names before this mapping. Intents and names that
    /// are not media types are not announced.
    #[must_use]
    pub(super) fn offered<'a>(
        mime_types: impl IntoIterator<Item = &'a str>,
    ) -> Vec<(ClipSource, RepresentationDescriptor)> {
        let mime_types = Intake::DEFAULT.admit_types(mime_types, |mime| *mime);
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

    /// What `FluiView.readDrop` answered, under `limits`: its `bytes`, or none
    /// when the representation was missing, the source failed, or it stopped
    /// after `read` bytes. `limits` checks a read that stopped past its byte
    /// ceiling, with the bytes read as a lower bound of the size; any other
    /// null answer means the source is gone.
    ///
    /// # Errors
    ///
    /// [`TransferError::TooLarge`] past `limits`, [`TransferError::SourceGone`]
    /// otherwise.
    pub(super) fn drop_bytes(
        bytes: Option<Vec<u8>>,
        read: u64,
        limits: TransferLimits,
    ) -> Result<Vec<u8>, TransferError> {
        let actual = bytes
            .as_ref()
            .map_or(read, |bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        limits.check(actual)?;
        bytes.ok_or(TransferError::SourceGone)
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
/// and the answer it returns. A taken drop retains only its representations'
/// lazy sources until the target concludes it; the Java view owns the clip and
/// its URI grant under the same offer key. Every host checks this state; the
/// JNI side around it reports events and performs the reads and releases.
///
/// A drop the target has not concluded keeps its offer, so drops overlap: a
/// new drag may enter, drop and conclude while an earlier drop is still being
/// read, and each conclusion retires its own offer alone.
#[cfg(any(target_os = "android", test))]
#[forbid(unsafe_code)]
mod drops {
    use std::collections::HashMap;
    use std::sync::Arc;

    use flui_foundation::{DataTransferId, geometry::Point};
    use flui_platform::DragDropEvent;
    use flui_platform::data_transfer::{
        DataTransferOffer, DropFeedback, RepresentationDescriptor, RepresentationIndex,
        TransferActions, TransferError,
    };

    use super::clip::{ClipSource, incoming_allowed, offered};
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

    #[derive(Default)]
    pub(super) struct Drops {
        table: OfferTable,
        /// The answer each live offer's target last cached.
        feedback: HashMap<DataTransferId, DropFeedback>,
        /// Where each representation of a hovering offer comes from at the drop.
        sources: HashMap<DataTransferId, Vec<ClipSource>>,
        /// Where each representation of a taken drop is read on request.
        dropped: HashMap<DataTransferId, Vec<ClipSource>>,
        /// The offer over the view and the actions its drag permits.
        hover: Option<(DataTransferId, TransferActions)>,
        /// The media types of the drag in progress, from `ACTION_DRAG_STARTED`.
        description: Vec<String>,
        /// What FLUI's own running drag advertised.
        own: Option<TransferActions>,
        /// The action FLUI's target took when FLUI's own drag dropped on it.
        local_drop: Option<TransferActions>,
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

        fn close(&mut self, id: DataTransferId) {
            let _ = self.table.retire(id);
            self.feedback.remove(&id);
            self.sources.remove(&id);
            self.dropped.remove(&id);
        }

        /// The key under which the view saves the hovering offer's clip before
        /// the drop wakes the owner, or zero when no offer hovers.
        pub(super) fn hover_key(&self) -> i64 {
            self.hover.map_or(0, |(id, _)| id.as_u64().cast_signed())
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

        /// `ACTION_DROP`: the event the drop queues and whether it was taken,
        /// from the target's cached feedback. `key` and `descriptors` are the
        /// offer key and `ClipDescription` the view received; a key for another
        /// hover changes nothing, and a description that no longer matches the
        /// offer refuses the drop. No item is read here: the view has already
        /// saved the clip under `key`, and requests read it until conclusion.
        pub(super) fn take_drop(
            &mut self,
            key: i64,
            position: (f64, f64),
            local: bool,
            descriptors: &[String],
        ) -> Option<(DragDropEvent, Option<DataTransferId>)> {
            let (id, allowed) = self.hover.take()?;
            if id.as_u64().cast_signed() != key {
                self.hover = Some((id, allowed));
                return None;
            }
            let described = offered(descriptors.iter().map(String::as_str));
            let matches = self.table.get(id).is_some_and(|representations| {
                representations.len() == described.len()
                    && representations
                        .iter()
                        .zip(&described)
                        .all(|(known, (_, current))| known.format == current.format)
            });
            let feedback = self.feedback.get(&id).copied().unwrap_or_default();
            let action = matches.then(|| feedback.resolve(allowed)).flatten();
            let Some(action) = action else {
                self.close(id);
                return Some((DragDropEvent::Exited { id }, None));
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
                Some(id),
            ))
        }

        /// The source and descriptor of representation `index` of dropped `id`.
        ///
        /// # Errors
        ///
        /// [`TransferError::SourceGone`] before the drop, while the clip's data
        /// is out of reach; [`TransferError::StaleOffer`] for a retired offer;
        /// [`TransferError::UnknownRepresentation`] past the offer's.
        pub(super) fn request(
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
            let source = sources
                .get(usize::from(index.0))
                .copied()
                .ok_or(TransferError::UnknownRepresentation { id, index })?;
            let descriptor = self
                .table
                .get(id)
                .and_then(|representations| representations.get(usize::from(index.0)))
                .cloned()
                .ok_or(TransferError::UnknownRepresentation { id, index })?;
            Ok((source, descriptor))
        }

        /// The target's answer for the live offer `id`, cached for the drop.
        pub(super) fn update_feedback(&mut self, id: DataTransferId, feedback: DropFeedback) {
            if let Some(cached) = self.feedback.get_mut(&id) {
                *cached = feedback;
            }
        }

        /// The target is done with the drop of `id`: its offer retires, and
        /// `true` only for a taken drop, so the view releases its saved clip
        /// and URI permissions exactly once. A stale, hovering or repeated
        /// conclusion has no Java clip to release.
        pub(super) fn conclude(&mut self, id: DataTransferId) -> bool {
            let release = self.dropped.contains_key(&id);
            self.close(id);
            release
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

    #[cfg(test)]
    mod tests;
}

// The JNI side, on Android alone.

#[cfg(target_os = "android")]
use std::sync::{Arc, Weak};

#[cfg(target_os = "android")]
use flui_foundation::DataTransferId;
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
    objects::{JObject, JObjectArray, JString},
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
use clip::{ClipSource, clip_parts, drop_bytes, source_end};
#[cfg(target_os = "android")]
use drops::{ACTION_DRAG_ENDED, Drops};

/// The size of the drag shadow, in logical pixels.
#[cfg(target_os = "android")]
const SHADOW: f64 = 48.0;

/// The platform's drag-and-drop transport (see the module doc).
#[cfg(target_os = "android")]
pub struct AndroidTransfer {
    view: Weak<JavaView>,
    drops: Mutex<Drops>,
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

    /// The key of the offer whose clip the view must save before `nativeDrop`
    /// wakes the owner, or zero when no offer hovers.
    fn hover_key(&self) -> i64 {
        self.drops.lock().hover_key()
    }

    /// `ACTION_DROP` of the clip the view saved under `key`: whether the target
    /// took it. Only its descriptors cross JNI here; every item stays in Java
    /// until a request reads it.
    fn drop_clip(
        &self,
        view: &JavaView,
        key: i64,
        position: (f64, f64),
        local: bool,
        descriptors: &[String],
    ) -> bool {
        let taken = self
            .drops
            .lock()
            .take_drop(key, position, local, descriptors);
        let Some((event, taken)) = taken else {
            return false;
        };
        view.push_drag(event);
        taken.is_some()
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
            drops: Mutex::default(),
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
        let found = self.drops.lock().request(id, representation);
        let (source, descriptor) = match found {
            Ok(found) => found,
            Err(error) => return TransferRequest::ready(Err(error)),
        };
        if source == ClipSource::Identity {
            return TransferRequest::ready(
                source.request(&descriptor, limits, |_, _| Err(TransferError::SourceGone)),
            )
            .enforce(limits);
        }
        let Some(view) = self.view.upgrade() else {
            return TransferRequest::ready(Err(TransferError::SourceGone));
        };
        let key = java_key(id);
        let (request, completer) = TransferRequest::pending();
        let spawned = std::thread::Builder::new()
            .name("flui-drop-read".into())
            .spawn(move || {
                let result = source.request(&descriptor, limits, |source, limits| {
                    read_drop(&view, key, source, limits)
                });
                completer.complete(result);
            });
        if let Err(error) = spawned {
            tracing::warn!(target: "flui_native::android", %error, "no thread to read a dropped clip");
        }
        request.enforce(limits)
    }

    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback) {
        self.drops.lock().update_feedback(id, feedback);
    }

    fn conclude_drop(&self, id: DataTransferId) {
        let concluded = self.drops.lock().conclude(id);
        if concluded && let Some(view) = self.view.upgrade() {
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
    }
}

/// The key `FluiView` holds a taken drop's clip and URI permissions under: the
/// offer's whole identity, generation included, so a concluded offer never
/// releases a later one's. Never zero, which the view reads as no offer.
#[cfg(target_os = "android")]
fn java_key(id: DataTransferId) -> i64 {
    id.as_u64().cast_signed()
}

/// One representation of the clip the view saved under `key`, read through its
/// bounded `readDrop` on the calling thread under `limits`.
#[cfg(target_os = "android")]
fn read_drop(
    view: &JavaView,
    key: i64,
    source: ClipSource,
    limits: TransferLimits,
) -> Result<Vec<u8>, TransferError> {
    let Some(target) = view.view() else {
        return Err(TransferError::SourceGone);
    };
    let read = view.attached(|env| {
        let count = env.new_long_array(1)?;
        let bytes = env
            .call_method(
                target.as_ref(),
                jni_str!("readDrop"),
                jni_sig!("(JIJ[J)[B"),
                &[
                    JValue::Long(key),
                    JValue::Int(source.code()),
                    JValue::Long(i64::try_from(limits.max_bytes).unwrap_or(i64::MAX)),
                    JValue::Object(&count),
                ],
            )?
            .l()?;
        let mut read = [0_i64];
        count.get_region(env, 0, &mut read)?;
        let read = u64::try_from(read[0]).unwrap_or(0);
        if bytes.is_null() {
            return Ok((None, read));
        }
        let bytes = env.cast_local::<jni::objects::JByteArray<'_>>(bytes)?;
        Ok((Some(env.convert_byte_array(&bytes)?), read))
    });
    match read {
        Ok((bytes, read)) => drop_bytes(bytes, read, limits),
        Err(_) => Err(TransferError::SourceGone),
    }
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
