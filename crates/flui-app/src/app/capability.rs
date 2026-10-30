//! The application's capability providers (ADR-0152): the built-in ones,
//! which this crate supplies as the composition root, and the registry a run
//! builds from them, the application's overrides and its plugins.

use std::{cell::RefCell, rc::Rc};

use flui_interaction::{ClipboardBackend, ClipboardCapability, ClipboardHandle};
use flui_native::NativeClipboard;
use flui_platform::{
    Unsupported, UnsupportedReason,
    capability::{
        Capability, CapabilityProvider, CapabilityRegistrar, Plugin, ProviderContext, Resolver,
    },
};
use flui_runtime::capability::{CapabilityConflict, CapabilityRegistry};

/// The slot the owner platform's clipboard is installed in for a loop's life.
pub(crate) type ClipboardSlot = Rc<RefCell<Option<Rc<dyn NativeClipboard>>>>;

/// The capability providers an application registers before it runs
/// (ADR-0152): its overrides, which replace a built-in provider, and its
/// plugins, which provide what neither the application nor the framework
/// does.
///
/// Built on the owner thread and handed to the run through
/// [`AppLaunch::capabilities`](crate::AppLaunch::capabilities) or
/// [`Application`](crate::Application)'s own builder methods. Two overrides or
/// two plugins for one capability, or two capabilities sharing a name, fail
/// the run with a [`CapabilityConflict`] before the platform starts.
///
/// ```rust,ignore
/// let capabilities = AppCapabilities::new()
///     .capability::<ClipboardCapability>(MyClipboard::default())
///     .plugin(MyPlugin);
/// ```
#[derive(Default)]
pub struct AppCapabilities {
    overrides: CapabilityRegistrar,
    plugins: Vec<Box<dyn Plugin>>,
}

impl AppCapabilities {
    /// No override and no plugin: the run gets the built-in providers alone.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Provide the capability `C` with `provider`, in place of the built-in
    /// provider and of any plugin's. A widget acquires it with
    /// `cx.capability::<C>()` in a lifecycle hook.
    #[must_use = "the builder returns the updated registrations; assign or chain it"]
    pub fn capability<C: Capability>(mut self, provider: impl CapabilityProvider<C>) -> Self {
        self.overrides.provide::<C>(provider);
        self
    }

    /// Register the providers of `plugin`, for the capabilities neither the
    /// application nor the framework provides.
    #[must_use = "the builder returns the updated registrations; assign or chain it"]
    pub fn plugin(mut self, plugin: impl Plugin) -> Self {
        self.plugins.push(Box::new(plugin));
        self
    }

    /// The registry of a run: these overrides, then the built-ins over
    /// `clipboard`, then the plugins. A plugin's `register` is user code, so
    /// the caller holds no borrow of the application runtime.
    ///
    /// # Errors
    ///
    /// The first [`CapabilityConflict`] among them.
    pub(crate) fn into_registry(
        self,
        clipboard: &ClipboardSlot,
    ) -> Result<CapabilityRegistry, CapabilityConflict> {
        let mut builder = CapabilityRegistry::builder()
            .overrides(self.overrides)
            .built_ins(built_ins(clipboard));
        for plugin in &self.plugins {
            builder = builder.plugin(plugin.as_ref());
        }
        builder.build()
    }
}

impl std::fmt::Debug for AppCapabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppCapabilities")
            .field("overrides", &self.overrides)
            .field("plugins", &self.plugins.len())
            .finish()
    }
}

/// The registry a run with no override and no plugin has.
pub(crate) fn built_in_registry(clipboard: &ClipboardSlot) -> CapabilityRegistry {
    CapabilityRegistry::builder()
        .built_ins(built_ins(clipboard))
        .build()
        .expect("BUG: the built-in providers name each capability once")
}

/// The providers this crate ships.
fn built_ins(clipboard: &ClipboardSlot) -> CapabilityRegistrar {
    let mut registrar = CapabilityRegistrar::new();
    registrar.provide::<ClipboardCapability>(PlatformClipboard {
        slot: Rc::clone(clipboard),
    });
    registrar
}

/// The built-in clipboard: the owner platform's, while one is installed.
struct PlatformClipboard {
    slot: ClipboardSlot,
}

impl CapabilityProvider<ClipboardCapability> for PlatformClipboard {
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<ClipboardHandle, Unsupported> {
        let native = self.slot.borrow().clone().ok_or_else(|| {
            // Between loops: no owner platform is installed.
            Unsupported::of::<ClipboardCapability>(UnsupportedReason::Unavailable)
        })?;
        Ok(ClipboardHandle::new(
            NativeBackend(native),
            cx.completions().clone(),
        ))
    }
}

/// A handle's view of the host's clipboard.
struct NativeBackend(Rc<dyn NativeClipboard>);

impl ClipboardBackend for NativeBackend {
    fn read_text(&self, answer: Resolver<Option<String>>) {
        self.0.read_text(answer);
    }

    fn write_text(&self, text: String) {
        self.0.write_text(text);
    }
}
