//! Android drag and drop through `FluiView` (ADR-0211, ADR-0247).
//!
//! [`AndroidTransfer`] is the platform's one [`DataTransferSource`]. A drag
//! over the view mints an offer from its `ClipDescription` when it enters,
//! answers the view at once on the Java main thread, and queues the
//! `DragDropEvent`s for the owner. The drop is decided from the feedback the
//! target cached, and copies the clip's text, HTML and URIs out before the
//! event returns; a URI's bytes are read through the content resolver when
//! requested, on a thread of their own. Each taken drop holds its own URI
//! permissions, keyed by its offer: concluding the drop retires the offer and
//! gives back those permissions alone, and a refused drop gives them back at
//! once.
//!
//! [`AndroidDragSource`] starts a system drag (`startDragAndDrop`, API 24)
//! when the runtime escalates one leaving the window. Android takes the whole
//! clip at the start, so every representation is served then; the view runs
//! the drag and reports its end, which reaches the request's end hook on the
//! owner thread.
//!
//! What a clip offers, holds and answers is decided in [`clip`] with no JNI
//! call, so every host compiles and checks it; the JNI side around it is
//! Android's alone.

/// The decidable half of the backend (ADR-0247, ADR-0211): how a
/// `DragEvent`'s clip maps to an offer, what a drop answers, what a FLUI drag
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
/// from the description's media types; the drop copies the text, HTML and
/// URIs out, and a URI's bytes are read when requested.
#[forbid(unsafe_code)]
mod clip {
    use std::sync::Arc;

    use flui_platform::data_transfer::{
        Mime, RepresentationDescriptor, TransferActions, TransferError, TransferFormat,
        TransferPayload,
    };
    use flui_platform::drag::{IDENTITY_ESSENCE, OsDragEnd};

    use crate::drag::offers::{PathStyle, TransferUri};

    /// `View.DRAG_FLAG_GLOBAL`: the drag may leave the source's window.
    pub(super) const DRAG_FLAG_GLOBAL: i32 = 1 << 8;
    /// `View.DRAG_FLAG_GLOBAL_URI_READ`: a target may read the drag's URIs.
    pub(super) const DRAG_FLAG_GLOBAL_URI_READ: i32 = 1;

    /// `ClipDescription.MIMETYPE_TEXT_INTENT`, which FLUI cannot redeem.
    pub(super) const MIMETYPE_TEXT_INTENT: &str = "text/vnd.android.intent";

    /// Where one announced representation comes from at the drop.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) enum ClipSource {
        /// The items' text, joined.
        Text,
        /// The first item's HTML.
        Html,
        /// Every item's URI.
        UriList,
        /// The bytes behind the first URI, read when requested.
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

    /// What a drop's clip carried, copied out while the drop event ran.
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub(super) struct DroppedClip {
        /// Every item's text, joined; `None` when no item had any.
        pub(super) text: Option<String>,
        /// The first item's HTML.
        pub(super) html: Option<String>,
        /// Every item's URI.
        pub(super) uris: Vec<String>,
    }

    /// One announced representation after the drop.
    #[derive(Debug, Clone)]
    pub(super) enum Held {
        /// Served from memory.
        Payload(TransferPayload),
        /// The bytes behind a URI, read when requested.
        Uri(HeldUri),
        /// Announced, but the clip did not carry it.
        Missing,
    }

    /// A dropped content URI whose bytes are read when requested.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct HeldUri {
        /// The content URI.
        pub(super) uri: String,
        /// The format the offer announced for its bytes.
        pub(super) format: TransferFormat,
    }

    impl HeldUri {
        /// The payload the URI's `bytes` resolve as: an image for an image
        /// offer, as announced, and raw bytes of the type's essence otherwise.
        #[must_use]
        pub(super) fn payload(&self, bytes: Vec<u8>) -> TransferPayload {
            crate::drag::offers::payload_of_bytes(&self.format, bytes)
        }
    }

    /// What `FluiView.readUri` answered, under `limit`: its `bytes`, or none
    /// when it stopped after `read` bytes. A read that stopped past the limit is
    /// too large, with the bytes it had read as a lower bound of the size; any
    /// other stop (no stream, an error) means the source is gone.
    ///
    /// # Errors
    ///
    /// [`TransferError::TooLarge`] past the limit, [`TransferError::SourceGone`]
    /// otherwise.
    pub(super) fn uri_bytes(
        bytes: Option<Vec<u8>>,
        read: u64,
        limit: u64,
    ) -> Result<Vec<u8>, TransferError> {
        let actual = bytes
            .as_ref()
            .map_or(read, |bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        if actual > limit {
            return Err(TransferError::TooLarge { actual, limit });
        }
        bytes.ok_or(TransferError::SourceGone)
    }

    /// What `source` of `descriptor` holds once `clip` dropped.
    #[must_use]
    pub(super) fn held(
        source: &ClipSource,
        descriptor: &RepresentationDescriptor,
        clip: &DroppedClip,
    ) -> Held {
        match source {
            ClipSource::Text => clip.text.clone().map_or(Held::Missing, |text| {
                Held::Payload(TransferPayload::Text(text))
            }),
            ClipSource::Html => clip.html.clone().map_or(Held::Missing, |html| {
                Held::Payload(TransferPayload::Html(html))
            }),
            ClipSource::UriList if clip.uris.is_empty() => Held::Missing,
            ClipSource::UriList => Held::Payload(TransferUri::list_payload(
                clip.uris.iter().cloned().map(TransferUri::Uri),
                PathStyle::Posix,
            )),
            ClipSource::Identity => Held::Payload(TransferPayload::Custom {
                mime: match &descriptor.format {
                    TransferFormat::Custom { mime } => mime.clone(),
                    _ => Mime::new(IDENTITY_ESSENCE),
                },
                bytes: Arc::from(Vec::new()),
            }),
            ClipSource::UriBytes => clip.uris.first().map_or(Held::Missing, |uri| {
                Held::Uri(HeldUri {
                    uri: uri.clone(),
                    format: descriptor.format.clone(),
                })
            }),
        }
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

// The JNI side, on Android alone.

#[cfg(target_os = "android")]
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

#[cfg(target_os = "android")]
use flui_foundation::{DataTransferId, geometry::Point};
#[cfg(target_os = "android")]
use flui_platform::{
    DragDropEvent, Unsupported, UnsupportedReason,
    data_transfer::{
        DataTransferOffer, DataTransferSource, DropFeedback, RepresentationDescriptor,
        RepresentationIndex, TransferActions, TransferError, TransferLimits, TransferPayload,
        TransferRequest,
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
    drag::offers::OfferTable,
    owner::android::java_view::{ActivityDragSource, JavaView, ViewEvent, ViewTransfer},
    owner::panic_boundary::contain_owner_callback,
    owner::slot::OwnerSlot,
    version::gate::Since,
    version::gate::android::{AndroidApi, DragSource},
};
#[cfg(target_os = "android")]
use clip::{
    ClipSource, DroppedClip, Held, clip_parts, held, incoming_allowed, offered, source_end,
    uri_bytes,
};

/// `DragEvent.ACTION_DRAG_STARTED`.
#[cfg(target_os = "android")]
const ACTION_DRAG_STARTED: i32 = 1;
/// `DragEvent.ACTION_DRAG_LOCATION`.
#[cfg(target_os = "android")]
const ACTION_DRAG_LOCATION: i32 = 2;
/// `DragEvent.ACTION_DRAG_ENDED`.
#[cfg(target_os = "android")]
const ACTION_DRAG_ENDED: i32 = 4;
/// `DragEvent.ACTION_DRAG_ENTERED`.
#[cfg(target_os = "android")]
const ACTION_DRAG_ENTERED: i32 = 5;
/// `DragEvent.ACTION_DRAG_EXITED`.
#[cfg(target_os = "android")]
const ACTION_DRAG_EXITED: i32 = 6;

/// The size of the drag shadow, in logical pixels.
#[cfg(target_os = "android")]
const SHADOW: f64 = 48.0;

#[derive(Default)]
#[cfg(target_os = "android")]
struct Offers {
    table: OfferTable,
    /// The answer each live offer's target last cached.
    feedback: HashMap<DataTransferId, DropFeedback>,
    /// Where each representation of a hovering offer comes from at the drop.
    sources: HashMap<DataTransferId, Vec<ClipSource>>,
    /// What each dropped offer holds, by representation index.
    held: HashMap<DataTransferId, Vec<Held>>,
    /// The offer over the view and the actions its drag permits.
    hover: Option<(DataTransferId, TransferActions)>,
    /// The media types of the drag in progress, from `ACTION_DRAG_STARTED`.
    description: Vec<String>,
    /// What FLUI's own running drag advertised.
    own: Option<TransferActions>,
    /// The action FLUI's target took when FLUI's own drag dropped on it.
    local_drop: Option<TransferActions>,
}

/// The platform's drag-and-drop transport (see the module doc).
#[cfg(target_os = "android")]
pub struct AndroidTransfer {
    view: Weak<JavaView>,
    offers: Mutex<Offers>,
}

#[cfg(target_os = "android")]
impl std::fmt::Debug for AndroidTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidTransfer").finish_non_exhaustive()
    }
}

#[cfg(target_os = "android")]
impl AndroidTransfer {
    fn new(view: Weak<JavaView>) -> Self {
        Self {
            view,
            offers: Mutex::default(),
        }
    }

    fn mint(offers: &mut Offers, mime_types: &[String]) -> DataTransferOffer {
        let (sources, representations): (Vec<_>, Vec<_>) =
            offered(mime_types.iter().map(String::as_str))
                .into_iter()
                .unzip();
        let representations: Arc<[RepresentationDescriptor]> = Arc::from(representations);
        let id = offers.table.mint(Arc::clone(&representations));
        offers.feedback.insert(id, DropFeedback::default());
        offers.sources.insert(id, sources);
        DataTransferOffer::new(id, representations)
    }

    fn close(offers: &mut Offers, id: DataTransferId) {
        let _ = offers.table.retire(id);
        offers.feedback.remove(&id);
        offers.sources.remove(&id);
        offers.held.remove(&id);
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
        let answer = {
            let mut offers = self.offers.lock();
            match action {
                ACTION_DRAG_STARTED => {
                    offers.description = mime_types.to_vec();
                    true
                }
                ACTION_DRAG_ENTERED => {
                    if let Some((id, _)) = offers.hover.take() {
                        events.push(DragDropEvent::Exited { id });
                        Self::close(&mut offers, id);
                    }
                    let types = if mime_types.is_empty() {
                        offers.description.clone()
                    } else {
                        mime_types.to_vec()
                    };
                    let allowed = incoming_allowed(offers.own.filter(|_| local));
                    let offer = Self::mint(&mut offers, &types);
                    offers.hover = Some((offer.id(), allowed));
                    events.push(DragDropEvent::Entered {
                        offer,
                        allowed,
                        position: None,
                    });
                    true
                }
                ACTION_DRAG_LOCATION => {
                    if let Some((id, allowed)) = offers.hover {
                        events.push(DragDropEvent::Moved {
                            id,
                            allowed,
                            position: Point::new(position.0, position.1),
                        });
                    }
                    true
                }
                ACTION_DRAG_EXITED => {
                    if let Some((id, _)) = offers.hover.take() {
                        events.push(DragDropEvent::Exited { id });
                        Self::close(&mut offers, id);
                    }
                    true
                }
                ACTION_DRAG_ENDED => {
                    if let Some((id, _)) = offers.hover.take() {
                        events.push(DragDropEvent::Exited { id });
                        Self::close(&mut offers, id);
                    }
                    offers.description.clear();
                    true
                }
                _ => false,
            }
        };
        for event in events {
            view.push_drag(event);
        }
        if action == ACTION_DRAG_ENDED && local {
            view.push_event(ViewEvent::DragEnded { result });
        }
        answer
    }

    /// `ACTION_DROP`: the [`java_key`] of the offer the drop is taken as
    /// ([`Self::take_drop`]), 0 when it is refused.
    fn drop_clip(
        &self,
        view: &JavaView,
        position: (f64, f64),
        local: bool,
        text: Option<String>,
        html: Option<String>,
        uris: Vec<String>,
    ) -> i64 {
        let clip = DroppedClip { text, html, uris };
        self.take_drop(view, position, local, &clip)
            .map_or(0, java_key)
    }

    fn set_own(&self, allowed: Option<TransferActions>) {
        let mut offers = self.offers.lock();
        offers.own = allowed;
        offers.local_drop = None;
    }

    fn take_local_drop(&self) -> Option<TransferActions> {
        self.offers.lock().local_drop.take()
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
        Arc::new(Self::new(view))
    }

    /// `ACTION_DROP`, on the Java main thread: the offer the drop is taken
    /// as, from the target's cached feedback, or `None` when it is refused.
    /// A taken drop holds what the clip carried until the target concludes
    /// it; the view holds the drop's URI permissions under the offer's
    /// [`java_key`] until then.
    fn take_drop(
        &self,
        view: &JavaView,
        position: (f64, f64),
        local: bool,
        clip: &DroppedClip,
    ) -> Option<DataTransferId> {
        let (event, taken) = {
            let mut offers = self.offers.lock();
            let (id, allowed) = offers.hover.take()?;
            let feedback = offers.feedback.get(&id).copied().unwrap_or_default();
            match feedback.resolve(allowed) {
                None => {
                    Self::close(&mut offers, id);
                    (DragDropEvent::Exited { id }, None)
                }
                Some(action) => {
                    let representations = offers.table.get(id).map(Arc::clone);
                    let sources = offers.sources.remove(&id).unwrap_or_default();
                    let values = representations
                        .iter()
                        .flat_map(|representations| representations.iter())
                        .zip(&sources)
                        .map(|(descriptor, source)| held(source, descriptor, clip))
                        .collect();
                    offers.held.insert(id, values);
                    if local && offers.own.is_some() {
                        offers.local_drop = Some(action);
                    }
                    (
                        DragDropEvent::Dropped {
                            id,
                            action,
                            position: Some(Point::new(position.0, position.1)),
                        },
                        Some(id),
                    )
                }
            }
        };
        view.push_drag(event);
        taken
    }
}

#[cfg(target_os = "android")]
impl DataTransferSource for AndroidTransfer {
    fn clipboard_offer(&self) -> Option<DataTransferOffer> {
        None
    }

    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest {
        let found = {
            let offers = self.offers.lock();
            let Some(held) = offers.held.get(&id) else {
                // Before the drop the clip's data is out of reach; a retired
                // offer is stale.
                return TransferRequest::ready(Err(if offers.sources.contains_key(&id) {
                    TransferError::SourceGone
                } else {
                    TransferError::StaleOffer(id)
                }));
            };
            match held.get(usize::from(representation.0)) {
                None => {
                    return TransferRequest::ready(Err(TransferError::UnknownRepresentation {
                        id,
                        index: representation,
                    }));
                }
                Some(found) => found.clone(),
            }
        };
        match found {
            Held::Payload(payload) => {
                let actual = payload.byte_len();
                TransferRequest::ready(if actual > limits.max_bytes {
                    Err(TransferError::TooLarge {
                        actual,
                        limit: limits.max_bytes,
                    })
                } else {
                    Ok(payload)
                })
            }
            Held::Missing => TransferRequest::ready(Err(TransferError::SourceGone)),
            Held::Uri(held) => {
                let Some(view) = self.view.upgrade() else {
                    return TransferRequest::ready(Err(TransferError::SourceGone));
                };
                let (request, completer) = TransferRequest::pending();
                let spawned = std::thread::Builder::new()
                    .name("flui-drop-read".into())
                    .spawn(move || {
                        let result = read_uri(&view, &held.uri, limits.max_bytes)
                            .map(|bytes| held.payload(bytes));
                        completer.complete(result);
                    });
                if let Err(error) = spawned {
                    tracing::warn!(target: "flui_native::android", %error, "no thread to read a dropped URI");
                }
                request
            }
        }
    }

    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback) {
        if let Some(cached) = self.offers.lock().feedback.get_mut(&id) {
            *cached = feedback;
        }
    }

    fn conclude_drop(&self, id: DataTransferId) {
        let concluded = {
            let mut offers = self.offers.lock();
            let live = offers.table.get(id).is_some();
            Self::close(&mut offers, id);
            live
        };
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

/// The key `FluiView` holds a taken drop's URI permissions under: the
/// offer's whole identity, generation included, so a concluded offer never
/// releases a later one's. Never zero, which the view reads as refused.
#[cfg(target_os = "android")]
fn java_key(id: DataTransferId) -> i64 {
    id.as_u64().cast_signed()
}

/// The bytes behind `uri`, read through the view's content resolver on the
/// calling thread.
#[cfg(target_os = "android")]
fn read_uri(view: &JavaView, uri: &str, limit: u64) -> Result<Vec<u8>, TransferError> {
    let Some(target) = view.view() else {
        return Err(TransferError::SourceGone);
    };
    let read = view.attached(|env| {
        let uri = env.new_string(uri)?;
        let count = env.new_long_array(1)?;
        let bytes = env
            .call_method(
                target.as_ref(),
                jni_str!("readUri"),
                jni_sig!("(Ljava/lang/String;J[J)[B"),
                &[
                    JValue::Object(&uri),
                    JValue::Long(i64::try_from(limit).unwrap_or(i64::MAX)),
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
        Ok((bytes, read)) => uri_bytes(bytes, read, limit),
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
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        provider(RepresentationIndex(index)).ok()
    }))
    .unwrap_or_else(|payload| {
        std::mem::forget(payload);
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
