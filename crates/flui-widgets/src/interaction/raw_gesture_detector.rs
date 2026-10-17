//! [`RawGestureDetector`] — mounts recognizers built by
//! [`GestureRecognizerFactory`] values and keeps each one alive across
//! rebuilds, reconfiguring it in place.

use std::{any::Any, any::TypeId, rc::Rc};

use flui_foundation::ViewKey;
use flui_interaction::pointer::{EventPropagation, PanZoomDispatch};
use flui_interaction::{
    GestureArena, GestureRecognizer, GestureSettingsProvider, PointerDispatch, cancel_all,
};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::WriterSource;
use flui_view::prelude::*;

use crate::{GestureArenaScope, Listener};

/// Builds one kind of recognizer for a [`RawGestureDetector`] and carries its
/// configuration.
///
/// The detector calls [`create`](Self::create) once, when the factory first
/// appears in its list, and [`update`](Self::update) with the live recognizer
/// on every later rebuild that lists a factory of the same type and key. A
/// rebuild therefore reconfigures the recognizer without replacing it, and a
/// contact the recognizer is tracking continues under the new configuration.
///
/// A recognizer that reads its configuration at event time keeps it in a
/// `Cell`/`RefCell` that `update` replaces; it releases that borrow before it
/// invokes a callback taken from it.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a gesture recognizer factory",
    label = "`RawGestureDetector` mounts recognizers only through a factory",
    note = "implement `GestureRecognizerFactory`: `create` builds the recognizer once, `update` reconfigures it on each rebuild"
)]
pub trait GestureRecognizerFactory: 'static {
    /// The recognizer this factory builds and reconfigures.
    type Recognizer: GestureRecognizer + 'static;

    /// Build the recognizer against the detector's arena, settings and writer.
    fn create(&self, cx: &RecognizerContext) -> Rc<Self::Recognizer>;

    /// Apply this factory's configuration to the recognizer an earlier factory
    /// of the same type and key created.
    fn update(&self, recognizer: &Self::Recognizer);
}

/// What a [`GestureRecognizerFactory`] builds a recognizer against: the
/// presentation's arena, the scope's gesture settings and the owner's writer
/// for signal writes from recognizer callbacks.
#[derive(Debug, Clone)]
pub struct RecognizerContext {
    arena: GestureArena,
    settings: GestureSettingsProvider,
    writer: WriterSource,
}

impl RecognizerContext {
    /// The arena of the enclosing [`GestureArenaScope`], checked against its
    /// settings.
    #[must_use]
    pub fn arena(&self) -> &GestureArena {
        &self.arena
    }

    /// The scope's gesture settings; read them per pointer kind at admission.
    #[must_use]
    pub fn settings(&self) -> &GestureSettingsProvider {
        &self.settings
    }

    /// The owner's writer: a recognizer callback opens a write through it to
    /// update signals.
    #[must_use]
    pub fn writer(&self) -> &WriterSource {
        &self.writer
    }
}

/// A recognizer built by a factory and kept by the detector's state.
trait LiveRecognizer {
    fn as_any(&self) -> &dyn Any;
    fn recognizer(&self) -> &dyn GestureRecognizer;
    fn attach(&self, listener: Listener, admission: Option<&ContactAdmission>) -> Listener;
}

impl<R: GestureRecognizer + 'static> LiveRecognizer for Rc<R> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn recognizer(&self) -> &dyn GestureRecognizer {
        &**self
    }

    fn attach(&self, listener: Listener, admission: Option<&ContactAdmission>) -> Listener {
        match admission {
            Some(admit) => {
                let admit = Rc::clone(admit);
                listener.recognizer_when(self, move |dispatch| admit(dispatch))
            }
            None => listener.recognizer(self),
        }
    }
}

/// Decides, from its Down, whether a contact reaches a detector's
/// recognizers at all; later events of an admitted contact always do.
pub(crate) type ContactAdmission = Rc<dyn Fn(PointerDispatch<'_>) -> bool>;

/// A factory with its recognizer type erased, so one detector holds several.
trait ErasedFactory {
    fn factory_type(&self) -> TypeId;
    fn create_live(&self, cx: &RecognizerContext) -> Box<dyn LiveRecognizer>;
    fn update_live(&self, live: &dyn LiveRecognizer);
}

#[diagnostic::do_not_recommend]
impl<F: GestureRecognizerFactory> ErasedFactory for F {
    fn factory_type(&self) -> TypeId {
        TypeId::of::<F>()
    }

    fn create_live(&self, cx: &RecognizerContext) -> Box<dyn LiveRecognizer> {
        Box::new(GestureRecognizerFactory::create(self, cx))
    }

    fn update_live(&self, live: &dyn LiveRecognizer) {
        let recognizer = live.as_any().downcast_ref::<Rc<F::Recognizer>>().expect(
            "BUG: a live recognizer is matched only to a factory of the type that created it",
        );
        GestureRecognizerFactory::update(self, recognizer);
    }
}

#[derive(Clone)]
struct FactoryEntry {
    key: Option<Rc<dyn ViewKey>>,
    factory: Rc<dyn ErasedFactory>,
}

struct Slot {
    factory_type: TypeId,
    key: Option<Rc<dyn ViewKey>>,
    live: Box<dyn LiveRecognizer>,
}

impl Slot {
    fn matches(&self, entry: &FactoryEntry) -> bool {
        self.factory_type == entry.factory.factory_type()
            && match (&self.key, &entry.key) {
                (None, None) => true,
                (Some(current), Some(incoming)) => current.key_eq(&**incoming),
                _ => false,
            }
    }
}

/// Mounts the recognizers its factories build on its child and keeps them
/// across rebuilds.
///
/// Each [`GestureRecognizerFactory`] is identified by its type and an optional
/// key ([`recognizer_with_key`](Self::recognizer_with_key)). On a rebuild a
/// factory whose type and key match a live recognizer reconfigures it through
/// [`update`](GestureRecognizerFactory::update); a contact in progress keeps
/// its recognizer, its arena membership and its accumulated state. A factory
/// that no longer appears cancels its recognizer; a new one creates its own.
/// Several factories of one type and key pair with the live recognizers in
/// list order.
///
/// Recognizers join the presentation arena of the enclosing
/// [`GestureArenaScope`] and receive the pointer stream in list order, so the
/// first factory's recognizer is the arena's front member. A scope whose
/// gesture settings change rebuilds every recognizer: the new ones are
/// committed before the old ones are cancelled.
///
/// The default hit behavior is [`DeferToChild`](HitTestBehavior::DeferToChild).
#[derive(Clone, StatefulView)]
pub struct RawGestureDetector {
    factories: Vec<FactoryEntry>,
    behavior: HitTestBehavior,
    /// Recognizers receive contacts only; native pan-zoom reaches them through
    /// `pan_zoom_claim`'s leaf-first walk instead.
    contact_input: bool,
    pan_zoom_claim: Option<PanZoomClaim>,
    admission: Option<ContactAdmission>,
    child: Child,
}

/// The arbitrated native pan-zoom handler a detector registers beside its
/// recognizers.
type PanZoomClaim = Rc<dyn Fn(PanZoomDispatch<'_>) -> EventPropagation>;

impl Default for RawGestureDetector {
    fn default() -> Self {
        Self {
            factories: Vec::new(),
            behavior: HitTestBehavior::DeferToChild,
            contact_input: false,
            pan_zoom_claim: None,
            admission: None,
            child: Child::empty(),
        }
    }
}

impl std::fmt::Debug for RawGestureDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawGestureDetector")
            .field("factories", &self.factories.len())
            .field("behavior", &self.behavior)
            .finish_non_exhaustive()
    }
}

impl RawGestureDetector {
    /// A detector with no recognizers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a recognizer, identified by its factory's type.
    #[must_use]
    pub fn recognizer(mut self, factory: impl GestureRecognizerFactory) -> Self {
        self.factories.push(FactoryEntry {
            key: None,
            factory: Rc::new(factory),
        });
        self
    }

    /// Add a recognizer identified by its factory's type and `key`, so that
    /// several recognizers of one type keep their identities when the list is
    /// reordered.
    #[must_use]
    pub fn recognizer_with_key(
        mut self,
        key: impl ViewKey,
        factory: impl GestureRecognizerFactory,
    ) -> Self {
        self.factories.push(FactoryEntry {
            key: Some(Rc::new(key)),
            factory: Rc::new(factory),
        });
        self
    }

    /// Set how the detector participates in hit-testing.
    #[must_use]
    pub fn behavior(mut self, behavior: HitTestBehavior) -> Self {
        self.behavior = behavior;
        self
    }

    /// Set the child the gestures are detected on.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// Route native pan-zoom through `claim`'s leaf-first walk, and only
    /// contacts to the recognizers, so a native pinch reaches one detector.
    #[must_use]
    pub(crate) fn pan_zoom_claim(
        mut self,
        claim: impl Fn(PanZoomDispatch<'_>) -> EventPropagation + 'static,
    ) -> Self {
        self.pan_zoom_claim = Some(Rc::new(claim));
        self
    }

    /// Deliver only contacts to the recognizers: scroll signals, hover and
    /// native pan-zoom stay with the listener's own handlers.
    #[must_use]
    pub(crate) fn contact_input(mut self) -> Self {
        self.contact_input = true;
        self
    }

    /// Hand a contact to the recognizers only when `admission` accepts its
    /// Down; a refused contact reaches whatever else is hit.
    #[must_use]
    pub(crate) fn admission(mut self, admission: ContactAdmission) -> Self {
        self.admission = Some(admission);
        self
    }
}

/// The live recognizers of a mounted [`RawGestureDetector`].
pub struct RawGestureDetectorState {
    factories: Vec<FactoryEntry>,
    context: Option<RecognizerContext>,
    slots: Vec<Slot>,
}

impl std::fmt::Debug for RawGestureDetectorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawGestureDetectorState")
            .field("recognizers", &self.slots.len())
            .finish_non_exhaustive()
    }
}

impl StatefulView for RawGestureDetector {
    type State = RawGestureDetectorState;

    fn create_state(&self) -> Self::State {
        RawGestureDetectorState {
            factories: self.factories.clone(),
            context: None,
            slots: Vec::new(),
        }
    }
}

fn context_of(ctx: &dyn LifecycleContext) -> RecognizerContext {
    RecognizerContext {
        arena: GestureArenaScope::of(ctx),
        settings: GestureArenaScope::settings_of(ctx),
        writer: ctx.writer_source(),
    }
}

fn create_slots(factories: &[FactoryEntry], cx: &RecognizerContext) -> Vec<Slot> {
    factories
        .iter()
        .map(|entry| Slot {
            factory_type: entry.factory.factory_type(),
            key: entry.key.clone(),
            live: entry.factory.create_live(cx),
        })
        .collect()
}

fn cancel_slots(slots: &[Slot]) {
    cancel_all(slots.iter().map(|slot| slot.live.recognizer()));
}

impl ViewState<RawGestureDetector> for RawGestureDetectorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let cx = context_of(ctx);
        self.slots = create_slots(&self.factories, &cx);
        self.context = Some(cx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let incoming = context_of(ctx);
        let changed = self
            .context
            .as_ref()
            .is_some_and(|current| current.settings != incoming.settings);
        if !changed {
            self.context = Some(incoming);
            return;
        }
        // The replacements are committed before the outgoing recognizers'
        // cancellation runs their callbacks.
        let replacements = create_slots(&self.factories, &incoming);
        self.context = Some(incoming);
        let outgoing = std::mem::replace(&mut self.slots, replacements);
        cancel_slots(&outgoing);
    }

    fn did_update_view(&mut self, _old_view: &RawGestureDetector, new_view: &RawGestureDetector) {
        let Some(cx) = self.context.clone() else {
            self.factories.clone_from(&new_view.factories);
            return;
        };
        // Pair each incoming factory with the first unclaimed live recognizer
        // of its type and key; no user code runs here.
        let mut claimed = vec![false; self.slots.len()];
        let pairing: Vec<Option<usize>> = new_view
            .factories
            .iter()
            .map(|entry| {
                let index = self
                    .slots
                    .iter()
                    .enumerate()
                    .position(|(index, slot)| !claimed[index] && slot.matches(entry))?;
                claimed[index] = true;
                Some(index)
            })
            .collect();
        // Factories without a live recognizer create one before anything is
        // committed: a panicking `create` leaves the mounted set intact.
        let mut created: Vec<Option<Box<dyn LiveRecognizer>>> = new_view
            .factories
            .iter()
            .zip(&pairing)
            .map(|(entry, paired)| paired.is_none().then(|| entry.factory.create_live(&cx)))
            .collect();

        let mut outgoing: Vec<Option<Slot>> = std::mem::take(&mut self.slots)
            .into_iter()
            .map(Some)
            .collect();
        let mut updates = Vec::new();
        for (index, entry) in new_view.factories.iter().enumerate() {
            let slot = match pairing[index] {
                Some(paired) => {
                    updates.push(index);
                    outgoing[paired]
                        .take()
                        .expect("BUG: each live recognizer is paired at most once")
                }
                None => Slot {
                    factory_type: entry.factory.factory_type(),
                    key: entry.key.clone(),
                    live: created[index]
                        .take()
                        .expect("BUG: an unpaired factory created its recognizer"),
                },
            };
            self.slots.push(slot);
        }
        self.factories.clone_from(&new_view.factories);
        let retired: Vec<Slot> = outgoing.into_iter().flatten().collect();
        cancel_slots(&retired);
        drop(retired);
        for index in updates {
            self.factories[index]
                .factory
                .update_live(&*self.slots[index].live);
        }
    }

    fn build(&self, view: &RawGestureDetector, _ctx: &dyn BuildContext) -> impl IntoView {
        let mut listener = Listener::new().behavior(view.behavior);
        if view.contact_input {
            listener = listener.contact_recognizers();
        }
        if let Some(claim) = view.pan_zoom_claim.clone() {
            listener = listener.on_pointer_pan_zoom_claim(move |dispatch| claim(dispatch));
        }
        let listener = self.slots.iter().fold(listener, |listener, slot| {
            slot.live.attach(listener, view.admission.as_ref())
        });
        match view.child.clone().into_inner() {
            Some(child) => listener.child(child),
            None => listener,
        }
    }

    fn dispose(&mut self) {
        let outgoing = std::mem::take(&mut self.slots);
        cancel_slots(&outgoing);
    }
}
