//! The application's capability providers (ADR-0152): the built-in ones,
//! which this crate supplies as the composition root, and the registry a run
//! builds from them, the application's overrides and its plugins.

use std::{cell::RefCell, rc::Rc};

use flui_interaction::{ClipboardBackend, ClipboardCapability, ClipboardHandle};
use flui_native::NativeClipboard;
use flui_platform::{
    Unsupported, UnsupportedReason,
    capability::{CapabilityProvider, CapabilityRegistrar, Plugin, ProviderContext, Resolver},
};
use flui_runtime::capability::{CapabilityConflict, CapabilityRegistry};

/// The slot the owner platform's clipboard is installed in for a loop's life.
pub(crate) type ClipboardSlot = Rc<RefCell<Option<Rc<dyn NativeClipboard>>>>;

/// What the application registers before it runs: its overrides and its
/// plugins, in the order it named them.
#[derive(Default)]
pub(crate) struct AppCapabilities {
    overrides: CapabilityRegistrar,
    plugins: Vec<Box<dyn Plugin>>,
}

impl AppCapabilities {
    pub(crate) fn override_with(&mut self, register: impl FnOnce(&mut CapabilityRegistrar)) {
        register(&mut self.overrides);
    }

    pub(crate) fn plugin(&mut self, plugin: Box<dyn Plugin>) {
        self.plugins.push(plugin);
    }

    /// The registry of a run: these overrides, then the built-ins over
    /// `clipboard`, then the plugins.
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
