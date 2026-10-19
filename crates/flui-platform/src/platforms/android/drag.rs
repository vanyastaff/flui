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

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use flui_foundation::DataTransferId;
use flui_foundation::geometry::Point;
use flui_platform_api::DragDropEvent;
use flui_platform_api::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, OfferRecord, OfferTable,
    RepresentationDescriptor, RepresentationIndex, TransferActions, TransferError, TransferLimits,
    TransferPayload, TransferRequest,
};
use flui_platform_api::drag::{
    DragError, OsDragEndHook, OsDragRequest, OsDragStart, PlatformDragSource,
};
use jni::objects::{JObject, JObjectArray, JString};
use jni::{Env, JValue, jni_sig, jni_str};
use parking_lot::Mutex;

use crate::shared::android_drag::{
    ACTION_DRAG_ENDED, ACTION_DRAG_ENTERED, ACTION_DRAG_EXITED, ACTION_DRAG_LOCATION,
    ACTION_DRAG_STARTED, ClipSource, DroppedClip, Held, clip_parts, held, incoming_allowed,
    offered, source_end, uri_bytes,
};
use crate::shared::os_version::Since;
use crate::shared::os_version::android::{AndroidApi, DragSource};
use crate::shared::owner_slot::OwnerSlot;
use crate::shared::panic_boundary::contain_owner_callback;

use super::java_view::{JavaView, ViewEvent};

/// The size of the drag shadow, in logical pixels.
const SHADOW: f64 = 48.0;

#[derive(Default)]
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
pub(crate) struct AndroidTransfer {
    view: Weak<JavaView>,
    offers: Mutex<Offers>,
}

impl AndroidTransfer {
    pub(super) fn new(view: Weak<JavaView>) -> Self {
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
        let id = offers
            .table
            .mint(OfferRecord::new(Arc::clone(&representations)));
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

    /// One `DragEvent` other than the drop, on the Java main thread: what the
    /// view answers. `position` is in logical pixels; `local` is whether this
    /// activity started the drag (for `ACTION_DRAG_ENDED`, whether the view
    /// did).
    pub(super) fn drag(
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

    /// `ACTION_DROP`, on the Java main thread: the offer the drop is taken
    /// as, from the target's cached feedback, or `None` when it is refused.
    /// A taken drop holds what the clip carried until the target concludes
    /// it; the view holds the drop's URI permissions under the offer's
    /// [`java_key`] until then.
    pub(super) fn drop_clip(
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
                    let representations = offers
                        .table
                        .get(id)
                        .map(|record| Arc::clone(record.representations()));
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

    fn set_own(&self, allowed: Option<TransferActions>) {
        let mut offers = self.offers.lock();
        offers.own = allowed;
        offers.local_drop = None;
    }

    fn take_local_drop(&self) -> Option<TransferActions> {
        self.offers.lock().local_drop.take()
    }
}

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
                let (request, completer) = TransferRequest::channel();
                let spawned = std::thread::Builder::new()
                    .name("flui-drop-read".into())
                    .spawn(move || {
                        let result = read_uri(&view, &held.uri, limits.max_bytes)
                            .map(|bytes| held.payload(bytes));
                        completer.complete(result);
                    });
                if let Err(error) = spawned {
                    tracing::warn!(target: "flui_platform::android", %error, "no thread to read a dropped URI");
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
pub(super) fn java_key(id: DataTransferId) -> i64 {
    id.as_u64().cast_signed()
}

/// The bytes behind `uri`, read through the view's content resolver on the
/// calling thread.
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
struct Running {
    allowed: TransferActions,
    on_end: OsDragEndHook,
}

/// A window's system drag source (see the module doc).
pub(crate) struct AndroidDragSource {
    view: Arc<JavaView>,
    /// `View.startDragAndDrop`, which the device has from API 24.
    start_drag: Option<Since<DragSource>>,
    running: OwnerSlot<Running>,
}

impl std::fmt::Debug for AndroidDragSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidDragSource").finish_non_exhaustive()
    }
}

impl AndroidDragSource {
    pub(super) fn new(view: Arc<JavaView>, api: AndroidApi) -> Self {
        Self {
            view,
            start_drag: api.since::<DragSource>(),
            running: OwnerSlot::new(),
        }
    }

    /// The system drag ended: `result` is whether a target took it. Owner
    /// thread.
    pub(super) fn finish(&self, result: bool) {
        let Some(Running { allowed, on_end }) = self.running.take() else {
            return;
        };
        let local = self.view.transfer().take_local_drop();
        self.view.transfer().set_own(None);
        let end = source_end(result, local, allowed);
        contain_owner_callback(|| on_end(end));
    }

    /// The window closed: a running drag ends cancelled. Owner thread.
    pub(super) fn retire(&self) {
        if let Some(Running { on_end, .. }) = self.running.take() {
            self.view.transfer().set_own(None);
            contain_owner_callback(|| on_end(flui_platform_api::drag::OsDragEnd::Cancelled));
        }
    }
}

/// Serve representation `index` through `provider`, containing a panic.
fn serve(
    provider: &mut flui_platform_api::drag::OsDragProvider,
    index: u16,
) -> Option<TransferPayload> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        provider(RepresentationIndex(index)).ok()
    }))
    .unwrap_or_else(|payload| {
        std::mem::forget(payload);
        contain_owner_callback(|| {
            tracing::error!(target: "flui_platform::android", "a drag's provider panicked");
        });
        None
    })
}

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

fn nullable<'local>(
    env: &mut Env<'local>,
    value: Option<&str>,
) -> jni::errors::Result<JString<'local>> {
    match value {
        Some(value) => env.new_string(value),
        None => Ok(JString::null()),
    }
}

impl PlatformDragSource for AndroidDragSource {
    fn start_timing(&self) -> OsDragStart {
        crate::shared::input_availability::DESKTOP_DRAG_START
    }

    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        let startable = self.start_drag.is_some()
            && self.view.is_ready()
            && self.running.is_occupied() == Some(false);
        let OsDragRequest {
            key,
            representations,
            allowed,
            mut provider,
            on_end,
            ..
        } = request;
        if !startable {
            contain_owner_callback(|| drop(provider));
            contain_owner_callback(|| drop(on_end));
            return Err(DragError::Unavailable);
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
            return Err(DragError::Unavailable);
        }
        self.view.transfer().set_own(Some(allowed));
        let session = match key.representation().format {
            flui_platform_api::data_transfer::TransferFormat::Custom { mime } => {
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
            tracing::info!(target: "flui_platform::android", started, "startDragAndDrop answered");
            Ok(started)
        }, refused);
        Ok(())
    }
}
