//! [`Localizations`] — resolves a [`Locale`] into localized resources and
//! publishes both to the subtree.
//!
//! Flutter parity: `widgets/localizations.dart` `Localizations` (oracle tag
//! `3.33.0-0.0.pre`, commit `88e87cd9` — the checked-out `packages/flutter`
//! tree; the plan's requested `3.44.0` tag was not present in the checkout).
//!
//! ## Sync-only v1 (documented divergences from the oracle)
//!
//! - **No async delegate loading.** The oracle's `LocalizationsDelegate.load`
//!   returns a `Future<T>` and `Localizations` defers the first frame while
//!   any delegate resolves asynchronously (`RendererBinding.deferFirstFrame`).
//!   FLUI's [`LocalizationsDelegate::load`] is synchronous, so resources are
//!   always available the instant `Localizations` is mounted — there is no
//!   "not yet loaded" state to model. A one-shot async delegate seam
//!   (parity with `RebuildHandle`/image-bridge precedent, ADR-0018) is a
//!   named follow-up, not implemented here.
//! - **No `Semantics` wrapper.** The oracle wraps the private scope in
//!   `Semantics(textDirection: ..., localeForSubtree: ...)` so the
//!   accessibility tree also carries locale/direction. FLUI's `Localizations`
//!   does not emit a `Semantics` node — a documented gap, not a silent one;
//!   closing it is a named follow-up once the semantics widget layer grows a
//!   `localeForSubtree`-equivalent property.
//! - **Coarse, locale-keyed rebuild.** The oracle's private
//!   `_LocalizationsScope.updateShouldNotify` compares `typeToResources` MAP
//!   IDENTITY: a new map is installed exactly when `Localizations.build()`
//!   decides to reload (locale changed, or a delegate's `shouldReload`
//!   fired), so *every* dependent rebuilds on that boundary — not a
//!   per-resource diff. This port's resources are a pure synchronous
//!   function of `locale` alone (delegates are fixed at construction, no
//!   `shouldReload` hook — see [`LocalizationsDelegate`]), so
//!   `update_should_notify` compares `locale` — the parity-equivalent signal
//!   for this simplified model. Do not "optimize" this into a per-key diff;
//!   it would change observable rebuild behavior.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use flui_types::platform::Locale;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};

use super::directionality::Directionality;
use super::widgets_localizations::{DefaultWidgetsLocalizations, WidgetsLocalizations};

/// A factory for a set of localized resources of type
/// [`Resources`](Self::Resources), loaded synchronously by a
/// [`Localizations`] widget.
///
/// Flutter parity: `LocalizationsDelegate<T>`
/// (`widgets/localizations.dart`), simplified to synchronous loading — see
/// the module docs for the full list of sync-only-v1 divergences.
pub trait LocalizationsDelegate: fmt::Debug {
    /// The localized-resource type this delegate produces. Retrieved later
    /// with `Localizations::of::<Self::Resources>`.
    type Resources: Send + Sync + 'static;

    /// Whether this delegate can produce resources for `locale`.
    fn is_supported(&self, locale: &Locale) -> bool;

    /// Produce the resources for `locale`. Only called when
    /// [`is_supported`](Self::is_supported) returned `true`.
    fn load(&self, locale: &Locale) -> Self::Resources;
}

/// Object-safe erasure of [`LocalizationsDelegate`], implemented for every
/// delegate via the blanket impl below. Confines the delegate's associated
/// `Resources` type behind a `TypeId` + `Arc<dyn Any + Send + Sync>` pair so
/// heterogeneous delegates can share one `Vec` — see
/// [`BoxedLocalizationsDelegate`].
trait ErasedLocalizationsDelegate: fmt::Debug + Send + Sync {
    fn resource_type_id(&self) -> TypeId;
    fn is_supported(&self, locale: &Locale) -> bool;
    fn load(&self, locale: &Locale) -> Arc<dyn Any + Send + Sync>;
}

impl<D> ErasedLocalizationsDelegate for D
where
    D: LocalizationsDelegate + Send + Sync + 'static,
{
    fn resource_type_id(&self) -> TypeId {
        TypeId::of::<D::Resources>()
    }

    fn is_supported(&self, locale: &Locale) -> bool {
        LocalizationsDelegate::is_supported(self, locale)
    }

    fn load(&self, locale: &Locale) -> Arc<dyn Any + Send + Sync> {
        Arc::new(LocalizationsDelegate::load(self, locale))
    }
}

/// A type-erased, cheaply-`Clone`-able [`LocalizationsDelegate`], ready to
/// sit in a [`Localizations::new`] delegate list alongside delegates that
/// produce unrelated resource types.
#[derive(Clone)]
pub struct BoxedLocalizationsDelegate(Arc<dyn ErasedLocalizationsDelegate>);

impl BoxedLocalizationsDelegate {
    /// Erase `delegate` for storage in a [`Localizations`] delegate list.
    #[must_use]
    pub fn new<D>(delegate: D) -> Self
    where
        D: LocalizationsDelegate + Send + Sync + 'static,
    {
        Self(Arc::new(delegate))
    }
}

impl<D> From<D> for BoxedLocalizationsDelegate
where
    D: LocalizationsDelegate + Send + Sync + 'static,
{
    fn from(delegate: D) -> Self {
        Self::new(delegate)
    }
}

impl fmt::Debug for BoxedLocalizationsDelegate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

/// A boxed [`WidgetsLocalizations`] trait object, `Sized` so it can be a
/// [`LocalizationsDelegate::Resources`] and stored in the `Localizations`
/// resource map. `Deref`s to the trait, so callers read it like the
/// underlying resource (`BoxedWidgetsLocalizations::of(ctx).text_direction()`).
#[derive(Debug)]
pub struct BoxedWidgetsLocalizations(Box<dyn WidgetsLocalizations>);

impl BoxedWidgetsLocalizations {
    /// Box `resources` for storage in a [`Localizations`] resource map.
    #[must_use]
    pub fn new(resources: impl WidgetsLocalizations + 'static) -> Self {
        Self(Box::new(resources))
    }

    /// The [`WidgetsLocalizations`] resource from the nearest ancestor
    /// [`Localizations`], registering a dependency.
    ///
    /// # Panics
    ///
    /// Panics if there is no `Localizations` ancestor providing this
    /// resource — impossible after mount once an app roots itself under a
    /// `Localizations` with a widgets-localizations delegate (see
    /// [`Localizations::new`]'s invariant). Use
    /// [`maybe_of`](Self::maybe_of) for a non-panicking variant.
    ///
    /// Flutter parity: `WidgetsLocalizations.of(context)`.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> Arc<Self> {
        Localizations::of::<Self>(ctx)
    }

    /// Look up the [`WidgetsLocalizations`] resource from the nearest
    /// ancestor [`Localizations`], registering a dependency. Returns `None`
    /// if there is no `Localizations` ancestor, or none of its delegates
    /// produce this resource type.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<Arc<Self>> {
        Localizations::maybe_of::<Self>(ctx)
    }
}

impl std::ops::Deref for BoxedWidgetsLocalizations {
    type Target = dyn WidgetsLocalizations;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

/// A [`LocalizationsDelegate`] that always resolves to
/// [`DefaultWidgetsLocalizations`] (US English, LTR), regardless of the
/// requested locale.
///
/// Flutter parity: `_WidgetsLocalizationsDelegate` /
/// `DefaultWidgetsLocalizations.delegate`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultWidgetsLocalizationsDelegate;

impl LocalizationsDelegate for DefaultWidgetsLocalizationsDelegate {
    type Resources = BoxedWidgetsLocalizations;

    fn is_supported(&self, _locale: &Locale) -> bool {
        true
    }

    fn load(&self, _locale: &Locale) -> Self::Resources {
        BoxedWidgetsLocalizations::new(DefaultWidgetsLocalizations)
    }
}

/// The resolved, immutable snapshot a [`LocalizationsScope`] publishes:
/// the locale it was resolved for, and every delegate's loaded resources
/// keyed by resource type. Cheap to `Clone` (two `Arc`/`String` clones).
#[derive(Clone)]
struct LocalizationsSnapshot {
    locale: Locale,
    resources: Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

/// Private inherited scope publishing the resolved [`LocalizationsSnapshot`]
/// to descendants. Never constructed directly — [`Localizations::build`]
/// is the sole producer.
///
/// Flutter parity: `_LocalizationsScope`.
#[derive(Clone)]
struct LocalizationsScope {
    snapshot: LocalizationsSnapshot,
    child: BoxedView,
}

impl InheritedView for LocalizationsScope {
    type Data = LocalizationsSnapshot;

    fn data(&self) -> &Self::Data {
        &self.snapshot
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.snapshot.locale != old.snapshot.locale
    }
}

impl_inherited_view!(LocalizationsScope);

/// Defines the [`Locale`] for its `child` and the localized resources the
/// child depends on, resolving them synchronously at build time.
///
/// Descendants read the resolved locale with [`Localizations::locale_of`],
/// and a delegate's resources with [`Localizations::of`] (typically via a
/// resource-specific convenience wrapper, e.g.
/// [`BoxedWidgetsLocalizations::of`]).
///
/// Flutter parity: `Localizations` (`widgets/localizations.dart`) — see the
/// module docs for the sync-only-v1 divergences (no async loading, no
/// `Semantics` wrapper).
#[derive(Clone, StatelessView)]
pub struct Localizations {
    locale: Locale,
    delegates: Arc<[BoxedLocalizationsDelegate]>,
    child: BoxedView,
}

impl fmt::Debug for Localizations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Localizations")
            .field("locale", &self.locale)
            .field("delegates", &self.delegates.len())
            .finish_non_exhaustive()
    }
}

impl Localizations {
    /// Create a `Localizations` that resolves `locale` through `delegates`
    /// for `child`'s subtree.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if no delegate produces
    /// [`BoxedWidgetsLocalizations`] — mirrors the oracle's constructor
    /// assert (`delegates.any((d) => d is LocalizationsDelegate<WidgetsLocalizations>)`).
    /// [`text_direction`](WidgetsLocalizations::text_direction) has nothing
    /// to resolve without it.
    #[must_use]
    pub fn new(
        locale: Locale,
        delegates: Vec<BoxedLocalizationsDelegate>,
        child: impl IntoView,
    ) -> Self {
        debug_assert!(
            delegates
                .iter()
                .any(|d| d.0.resource_type_id() == TypeId::of::<BoxedWidgetsLocalizations>()),
            "BUG: Localizations::new requires at least one delegate producing \
             BoxedWidgetsLocalizations (add DefaultWidgetsLocalizationsDelegate, or the \
             flui-localizations GlobalWidgetsLocalizations delegate)"
        );
        Self {
            locale,
            delegates: delegates.into(),
            child: child.into_view().boxed(),
        }
    }

    /// The [`Locale`] of the [`Localizations`] ancestor for `ctx`,
    /// registering a dependency.
    ///
    /// # Panics
    ///
    /// Panics if there is no `Localizations` ancestor. Use
    /// [`maybe_locale_of`](Self::maybe_locale_of) for a non-panicking
    /// variant.
    ///
    /// Flutter parity: `Localizations.localeOf(context)`.
    #[must_use]
    pub fn locale_of(ctx: &dyn BuildContext) -> Locale {
        Self::maybe_locale_of(ctx).expect(
            "BUG: Localizations::locale_of called with no Localizations ancestor in the tree",
        )
    }

    /// The [`Locale`] of the [`Localizations`] ancestor for `ctx`,
    /// registering a dependency. Returns `None` if there is no
    /// `Localizations` ancestor.
    ///
    /// Flutter parity: `Localizations.maybeLocaleOf(context)`.
    #[must_use]
    pub fn maybe_locale_of(ctx: &dyn BuildContext) -> Option<Locale> {
        ctx.depend_on::<LocalizationsScope, _>(|scope| scope.snapshot.locale.clone())
    }

    /// Retrieve the resource of type `R` produced by some delegate on the
    /// nearest ancestor [`Localizations`], registering a dependency.
    ///
    /// Two distinct absences collapse into `None` here: no `Localizations`
    /// ancestor at all, or an ancestor whose delegates never produce `R`.
    /// Both are permanently-reachable "no provider" states in this
    /// sync-only model (unlike the oracle's async path, there is no
    /// "not yet loaded" state once mounted — see the module docs).
    ///
    /// # Panics
    ///
    /// Never panics; see [`of`](Self::of) for the panicking variant.
    #[must_use]
    pub fn maybe_of<R: Send + Sync + 'static>(ctx: &dyn BuildContext) -> Option<Arc<R>> {
        let erased = ctx
            .depend_on::<LocalizationsScope, _>(|scope| {
                scope.snapshot.resources.get(&TypeId::of::<R>()).cloned()
            })
            .flatten()?;
        // Retrieves the delegate-declared `R` from the type-erased
        // `Arc<dyn Any + Send + Sync>` resource map by the caller's
        // requested resource type — the sole sanctioned resource-map
        // downcast site for the localizations substrate, mirroring
        // ADR-0019's `RouteRecord::did_complete` boundary.
        erased.downcast::<R>().ok() // PORT-CHECK-OK-DOWNCAST: localizations resource-map lookup by caller-requested type, see this fn's doc
    }

    /// Retrieve the resource of type `R` produced by some delegate on the
    /// nearest ancestor [`Localizations`], registering a dependency.
    ///
    /// # Panics
    ///
    /// Panics naming `R` if there is no `Localizations` ancestor, or none of
    /// its delegates produce `R`. Use [`maybe_of`](Self::maybe_of) for a
    /// non-panicking variant.
    #[must_use]
    pub fn of<R: Send + Sync + 'static>(ctx: &dyn BuildContext) -> Arc<R> {
        Self::maybe_of::<R>(ctx).unwrap_or_else(|| {
            panic!(
                "BUG: Localizations::of::<{}> called with no delegate providing this resource \
                 on the tree — add a LocalizationsDelegate<Resources = {0}> to the ancestor \
                 Localizations, or use Localizations::maybe_of for a non-panicking lookup",
                std::any::type_name::<R>()
            )
        })
    }
}

impl StatelessView for Localizations {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Sync-only v1: no deferred-first-frame gap — see module docs.
        let mut resources: HashMap<TypeId, Arc<dyn Any + Send + Sync>> =
            HashMap::with_capacity(self.delegates.len());
        for delegate in self.delegates.iter() {
            // Only the first delegate of a given resource type loads —
            // oracle parity (`_loadAll`'s `if (!types.contains(delegate.type))`).
            let type_id = delegate.0.resource_type_id();
            if resources.contains_key(&type_id) || !delegate.0.is_supported(&self.locale) {
                continue;
            }
            resources.insert(type_id, delegate.0.load(&self.locale));
        }

        // Same sanctioned site as `Localizations::maybe_of` (this fn's own
        // doc marks the reason) — resolves the text direction `build` wraps
        // its child in.
        let widgets_localizations = resources
            .get(&TypeId::of::<BoxedWidgetsLocalizations>())
            .cloned()
            .and_then(|erased| erased.downcast::<BoxedWidgetsLocalizations>().ok()) // PORT-CHECK-OK-DOWNCAST: localizations resource-map lookup, see Localizations::maybe_of's doc
            .expect(
                "BUG: Localizations::new's debug_assert should have caught a missing \
                 BoxedWidgetsLocalizations delegate before this build ran",
            );
        let text_direction = widgets_localizations.text_direction();

        LocalizationsScope {
            snapshot: LocalizationsSnapshot {
                locale: self.locale.clone(),
                resources: Arc::new(resources),
            },
            child: Directionality::new(text_direction, self.child.clone()).boxed(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use flui_types::typography::TextDirection;

    use super::*;
    use crate::SizedBox;
    use crate::test_harness::mount;

    fn scope(locale: Locale) -> LocalizationsScope {
        LocalizationsScope {
            snapshot: LocalizationsSnapshot {
                locale,
                resources: Arc::new(HashMap::new()),
            },
            child: SizedBox::shrink().boxed(),
        }
    }

    #[test]
    fn scope_update_should_notify_same_locale_is_false() {
        let a = scope(Locale::en_us());
        let b = scope(Locale::en_us());
        assert!(
            !a.update_should_notify(&b),
            "an unchanged locale must not notify dependents"
        );
    }

    #[test]
    fn scope_update_should_notify_different_locale_is_true() {
        let a = scope(Locale::fr_fr());
        let b = scope(Locale::en_us());
        assert!(
            a.update_should_notify(&b),
            "a changed locale must notify every dependent (coarse rebuild is parity — see the \
             module docs)"
        );
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Marker(u32);

    #[derive(Debug, Clone, Copy, Default)]
    struct MarkerDelegate;

    impl LocalizationsDelegate for MarkerDelegate {
        type Resources = Marker;

        fn is_supported(&self, _locale: &Locale) -> bool {
            true
        }

        fn load(&self, _locale: &Locale) -> Self::Resources {
            Marker(7)
        }
    }

    /// A resource type no delegate in these tests ever provides — the
    /// "permanently reachable" absence `Localizations::maybe_of` must
    /// report as `None` (distinct from the sync-only model's "not-yet-loaded"
    /// absence, which cannot occur after mount — see the module docs).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct NotProvided;

    fn widgets_only_delegates() -> Vec<BoxedLocalizationsDelegate> {
        vec![BoxedLocalizationsDelegate::new(
            DefaultWidgetsLocalizationsDelegate,
        )]
    }

    fn widgets_and_marker_delegates() -> Vec<BoxedLocalizationsDelegate> {
        vec![
            BoxedLocalizationsDelegate::new(DefaultWidgetsLocalizationsDelegate),
            BoxedLocalizationsDelegate::new(MarkerDelegate),
        ]
    }

    #[test]
    fn new_wires_locale_and_child() {
        let localizations = Localizations::new(
            Locale::en_us(),
            widgets_only_delegates(),
            SizedBox::shrink(),
        );
        assert_eq!(localizations.locale, Locale::en_us());
    }

    #[test]
    #[should_panic(expected = "requires at least one delegate producing BoxedWidgetsLocalizations")]
    fn new_panics_without_a_widgets_localizations_delegate() {
        // `Localizations::new`'s `debug_assert!` runs at construction time,
        // called directly here (not through `mount`'s build-panic boundary,
        // which would swallow it into an `ErrorView` — see
        // `crates/flui-widgets/tests/theme.rs` for that documented
        // limitation), so `#[should_panic]` observes it.
        let _ = Localizations::new(Locale::en_us(), Vec::new(), SizedBox::shrink());
    }

    // `Localizations::of`/`locale_of`'s no-ancestor panic path is
    // deliberately not exercised via `mount` + `#[should_panic]`: a panic
    // inside `build()` is caught by the framework's build-error boundary
    // (an `ErrorView` is substituted) rather than unwinding out to the test
    // — the same limitation `tests/theme.rs` documents for `Theme::of`. The
    // success and `None` paths below are what a mounted tree can actually
    // observe; the panic message itself is reviewed as part of the source.

    /// A boxed probe closure a [`Capture`] runs against its live
    /// `BuildContext` during `build()`.
    type ReadFn<T> = Arc<dyn Fn(&dyn BuildContext) -> T + Send + Sync>;

    /// Captures whatever a probe closure computes from a live `BuildContext`
    /// during `build()`, once. Generic over the captured type so each test
    /// below states only what it reads, not a bespoke probe widget.
    #[derive(Clone, StatelessView)]
    struct Capture<T: Clone + Send + Sync + 'static> {
        read: ReadFn<T>,
        captured: Arc<Mutex<Option<T>>>,
    }

    impl<T: Clone + Send + Sync + 'static> fmt::Debug for Capture<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Capture").finish_non_exhaustive()
        }
    }

    impl<T: Clone + Send + Sync + 'static> StatelessView for Capture<T> {
        fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
            *self.captured.lock().expect("test mutex poisoned") = Some((self.read)(ctx));
            SizedBox::shrink()
        }
    }

    fn capture<T: Clone + Send + Sync + 'static>(
        read: impl Fn(&dyn BuildContext) -> T + Send + Sync + 'static,
    ) -> (Capture<T>, Arc<Mutex<Option<T>>>) {
        let captured = Arc::new(Mutex::new(None));
        (
            Capture {
                read: Arc::new(read),
                captured: Arc::clone(&captured),
            },
            captured,
        )
    }

    #[test]
    fn locale_of_resolves_the_mounted_locale() {
        let (probe, captured) = capture(Localizations::locale_of);
        let _harness =
            mount(Localizations::new(Locale::en_us(), widgets_only_delegates(), probe).boxed());
        assert_eq!(
            captured.lock().expect("test mutex poisoned").clone(),
            Some(Locale::en_us())
        );
    }

    #[test]
    fn of_returns_a_delegate_provided_resource() {
        let (probe, captured) = capture(Localizations::maybe_of::<Marker>);
        let _harness = mount(
            Localizations::new(Locale::en_us(), widgets_and_marker_delegates(), probe).boxed(),
        );
        assert_eq!(
            captured
                .lock()
                .expect("test mutex poisoned")
                .clone()
                .flatten()
                .as_deref()
                .copied(),
            Some(Marker(7))
        );
    }

    #[test]
    fn maybe_of_returns_none_when_no_delegate_provides_the_type() {
        let (probe, captured) = capture(Localizations::maybe_of::<NotProvided>);
        let _harness = mount(
            // Only the widgets delegate — nothing produces `NotProvided`.
            Localizations::new(Locale::en_us(), widgets_only_delegates(), probe).boxed(),
        );
        assert!(
            captured
                .lock()
                .expect("test mutex poisoned")
                .clone()
                .flatten()
                .is_none(),
            "maybe_of::<NotProvided> must report None, not panic or fabricate a value"
        );
    }

    #[test]
    fn boxed_widgets_localizations_of_resolves_the_default_ltr_resource() {
        let (probe, captured) = capture(|ctx| BoxedWidgetsLocalizations::of(ctx).text_direction());
        let _harness =
            mount(Localizations::new(Locale::en_us(), widgets_only_delegates(), probe).boxed());
        assert_eq!(
            captured.lock().expect("test mutex poisoned").clone(),
            Some(TextDirection::Ltr)
        );
    }

    // ------------------------------------------------------------------
    // `of`'s panic path
    // ------------------------------------------------------------------
    //
    // Unlike a panic inside `build()` (caught by the framework's build-error
    // boundary and substituted with an `ErrorView` — see
    // `crates/flui-widgets/tests/theme.rs`'s documented limitation for
    // `Theme::of`), a panic inside `ViewState::init_state` runs OUTSIDE that
    // `catch_unwind` (`element/behavior.rs`'s `StatefulBehavior::build_into_views`
    // calls `self.state.init_state(ctx)` before the build closure it wraps),
    // so it propagates all the way out to `mount()` — genuinely observable
    // with `#[should_panic]`, not a self-authored guess about the message.

    /// A boxed `init_state` probe action — see [`InitStatePanicProbe`].
    type InitStateAction = Arc<dyn Fn(&dyn BuildContext) + Send + Sync>;

    /// A probe whose `init_state` calls a caller-supplied closure against a
    /// live, no-Localizations-ancestor `BuildContext` — used to drive
    /// `Localizations::of`'s panic path for real.
    #[derive(Clone, StatefulView)]
    struct InitStatePanicProbe {
        run: InitStateAction,
    }

    struct InitStatePanicProbeState {
        run: InitStateAction,
    }

    impl StatefulView for InitStatePanicProbe {
        type State = InitStatePanicProbeState;

        fn create_state(&self) -> Self::State {
            InitStatePanicProbeState {
                run: Arc::clone(&self.run),
            }
        }
    }

    impl ViewState<InitStatePanicProbe> for InitStatePanicProbeState {
        fn init_state(&mut self, ctx: &dyn BuildContext) {
            (self.run)(ctx);
        }

        fn build(&self, _view: &InitStatePanicProbe, _ctx: &dyn BuildContext) -> impl IntoView {
            SizedBox::shrink()
        }
    }

    #[test]
    #[should_panic(expected = "Localizations::locale_of called with no Localizations ancestor")]
    fn locale_of_panics_with_no_localizations_ancestor() {
        let probe = InitStatePanicProbe {
            run: Arc::new(|ctx| {
                let _ = Localizations::locale_of(ctx);
            }),
        };
        let _harness = mount(probe.boxed());
    }

    #[test]
    #[should_panic(
        expected = "Localizations::of::<flui_widgets::localization::localizations::tests::NotProvided>"
    )]
    fn of_panic_message_names_the_requested_type() {
        let probe = InitStatePanicProbe {
            run: Arc::new(|ctx| {
                let _ = Localizations::of::<NotProvided>(ctx);
            }),
        };
        let _harness = mount(probe.boxed());
    }
}
