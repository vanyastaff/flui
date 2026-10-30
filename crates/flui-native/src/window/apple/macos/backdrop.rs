//! The material AppKit draws behind a macOS window's content
//! ([`AppKitWindow::set_backdrop`]): one of `NSVisualEffectView`'s materials,
//! or Liquid Glass, `NSGlassEffectView`.
//!
//! Either view becomes the window's content view with FLUI's view inside it:
//! FLUI's view keeps carrying input, the input method, frame requests and the
//! surface, and stays the first responder. `NSGlassEffectView` exists from
//! macOS 26; it is looked up by name when asked for, so an earlier system
//! answers `Unsupported` with `NotOnThisDevice` and needs no version gate.
//! Only what AppKit applies is an input: a material (with the blend behind the
//! window and the active state a window backdrop needs), and the glass's
//! style, tint and corner radius.

use std::sync::atomic::Ordering;

use flui_platform::{Unsupported, UnsupportedReason};
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{MainThreadMarker, class, msg_send};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState,
};
use objc2_foundation::NSRect;

use super::ext::AppKitWindow;
use super::{BObjC, MacOSWindow, NIL, ObjcId};

impl MacOSWindow {
    /// `WindowCommand::SetMaterial` on an AppKit window: `System` is
    /// `NSVisualEffectView`'s window-background material behind FLUI's view,
    /// `Opaque` takes the backdrop away.
    pub(super) fn apply_material(
        &self,
        material: flui_platform::WindowMaterial,
    ) -> Result<(), flui_platform::ApplyError> {
        let view = AppKitWindow(self);
        material_applied(match material {
            flui_platform::WindowMaterial::System => {
                view.set_backdrop(MacOSBackdrop::Material(MacOSMaterial::WindowBackground))
            }
            flui_platform::WindowMaterial::Opaque => view.clear_backdrop(),
        })
    }
}

/// What the window answers for AppKit's `outcome`: a closed window is
/// `Closed`, and a backdrop AppKit did not take is a failure of the system,
/// `ApplyError::Platform`, not a capability the window lacks.
fn material_applied(
    outcome: Result<(), MacOSBackdropError>,
) -> Result<(), flui_platform::ApplyError> {
    match outcome {
        Ok(()) => Ok(()),
        Err(MacOSBackdropError::Unsupported(refused))
            if refused.reason == UnsupportedReason::NoWindow =>
        {
            Err(flui_platform::ApplyError::Closed)
        }
        Err(error) => Err(flui_platform::ApplyError::failed(error.to_string())),
    }
}

/// What AppKit draws behind a window's content
/// ([`AppKitWindow::set_backdrop`]). FLUI's content shows it where it draws
/// transparent pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum MacOSBackdrop {
    /// One of `NSVisualEffectView`'s materials, blended with what is behind
    /// the window.
    Material(MacOSMaterial),
    /// Liquid Glass (`NSGlassEffectView`, macOS 26).
    Glass(LiquidGlass),
}

/// One of `NSVisualEffectView`'s materials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MacOSMaterial {
    /// Behind a window's main content (`contentBackground`).
    ContentBackground,
    /// A window's background (`windowBackground`).
    WindowBackground,
    /// Under everything else in the window (`underWindowBackground`).
    UnderWindowBackground,
    /// A sidebar's (`sidebar`).
    Sidebar,
    /// A menu's (`menu`).
    Menu,
    /// A popover's (`popover`).
    Popover,
    /// A heads-up display's: dark and translucent (`hudWindow`).
    HudWindow,
}

impl MacOSMaterial {
    fn material(self) -> NSVisualEffectMaterial {
        match self {
            Self::ContentBackground => NSVisualEffectMaterial::ContentBackground,
            Self::WindowBackground => NSVisualEffectMaterial::WindowBackground,
            Self::UnderWindowBackground => NSVisualEffectMaterial::UnderWindowBackground,
            Self::Sidebar => NSVisualEffectMaterial::Sidebar,
            Self::Menu => NSVisualEffectMaterial::Menu,
            Self::Popover => NSVisualEffectMaterial::Popover,
            Self::HudWindow => NSVisualEffectMaterial::HUDWindow,
        }
    }
}

/// Liquid Glass behind a window's content: what `NSGlassEffectView` applies.
/// A `None` input is AppKit's own default.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct LiquidGlass {
    /// How much of what is behind the glass shows through.
    pub style: GlassStyle,
    /// A colour the glass is tinted with.
    pub tint: Option<GlassTint>,
    /// The radius of the glass's corners, in points: finite and not negative.
    pub corner_radius: Option<f64>,
}

impl LiquidGlass {
    /// Set the glass's style.
    #[must_use]
    pub const fn with_style(mut self, style: GlassStyle) -> Self {
        self.style = style;
        self
    }

    /// Tint the glass.
    #[must_use]
    pub const fn with_tint(mut self, tint: GlassTint) -> Self {
        self.tint = Some(tint);
        self
    }

    /// Round the glass's corners, in points.
    #[must_use]
    pub const fn with_corner_radius(mut self, radius: f64) -> Self {
        self.corner_radius = Some(radius);
        self
    }
}

/// How much of what is behind Liquid Glass shows through
/// (`NSGlassEffectViewStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum GlassStyle {
    /// The standard glass.
    #[default]
    Regular,
    /// Clearer glass, for content behind it that should stay legible.
    Clear,
}

impl GlassStyle {
    /// The `NSGlassEffectViewStyle` value, an `NSInteger`.
    fn value(self) -> isize {
        match self {
            Self::Regular => 0,
            Self::Clear => 1,
        }
    }
}

/// An sRGB colour Liquid Glass is tinted with, each channel in `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassTint {
    red: f64,
    green: f64,
    blue: f64,
    alpha: f64,
}

impl GlassTint {
    /// A tint from sRGB channels and alpha; `None` unless each is finite and in
    /// `0.0..=1.0`.
    #[must_use]
    pub fn try_new(red: f64, green: f64, blue: f64, alpha: f64) -> Option<Self> {
        [red, green, blue, alpha]
            .iter()
            .all(|channel| (0.0..=1.0).contains(channel))
            .then_some(Self {
                red,
                green,
                blue,
                alpha,
            })
    }
}

/// Why AppKit did not change a window's backdrop
/// ([`AppKitWindow::set_backdrop`], [`AppKitWindow::clear_backdrop`]). The
/// window is then as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MacOSBackdropError {
    /// Not here, in the contract's vocabulary: the window is closed
    /// ([`UnsupportedReason::NoWindow`]), or this macOS has no
    /// `NSGlassEffectView`, so no Liquid Glass before macOS 26
    /// ([`UnsupportedReason::NotOnThisDevice`]).
    #[error(transparent)]
    Unsupported(#[from] Unsupported),
    /// The window is not on the AppKit main thread's owner lane, where its
    /// views live.
    #[error("the window is not on the AppKit main thread's owner lane")]
    NotOnTheMainLane,
    /// AppKit did not create the view that draws the backdrop.
    #[error("AppKit did not create the backdrop's view")]
    NoBackdropView,
    /// The window's content view is neither FLUI's view nor a backdrop
    /// holding it: something else replaced it, and it is left alone.
    #[error("the window's content view is not FLUI's")]
    ForeignContentView,
    /// The glass's corner radius is not finite or is negative.
    #[error("the corner radius is not finite or is negative")]
    InvalidCornerRadius,
}

impl MacOSBackdropError {
    /// The window is closed.
    const CLOSED: Self = Self::Unsupported(Unsupported::new(UnsupportedReason::NoWindow));
    /// This macOS has no Liquid Glass.
    const NO_GLASS: Self = Self::Unsupported(Unsupported::new(UnsupportedReason::NotOnThisDevice));
}

/// A subview that tracks its superview's size.
const FILL: NSAutoresizingMaskOptions =
    NSAutoresizingMaskOptions::ViewWidthSizable.union(NSAutoresizingMaskOptions::ViewHeightSizable);

/// What the window's content view is.
enum Content {
    /// FLUI's view: no backdrop.
    Flui,
    /// An `NSVisualEffectView` holding FLUI's view.
    Material(ObjcId),
    /// An `NSGlassEffectView` holding FLUI's view.
    Glass(ObjcId),
}

impl AppKitWindow<'_> {
    /// Draw `backdrop` behind the window's content, in place of the backdrop
    /// it had.
    ///
    /// An `NSVisualEffectView` or an `NSGlassEffectView` becomes the window's
    /// content view and FLUI's view its content, filling it: FLUI's view keeps
    /// its input, its surface and the first-responder status. A material set
    /// over a material changes that view in place; glass is a new view each
    /// time, so an input left `None` is AppKit's default.
    ///
    /// # Errors
    /// [`MacOSBackdropError`]: the window is closed or off the AppKit main
    /// lane, Liquid Glass before macOS 26, a corner radius that is not finite
    /// and not negative, a content view something else put there, or a view
    /// AppKit did not create. The window is then as it was.
    pub fn set_backdrop(self, backdrop: MacOSBackdrop) -> Result<(), MacOSBackdropError> {
        if let MacOSBackdrop::Glass(LiquidGlass {
            corner_radius: Some(radius),
            ..
        }) = backdrop
            && !(radius.is_finite() && radius >= 0.0)
        {
            return Err(MacOSBackdropError::InvalidCornerRadius);
        }
        let window = self.0;
        if !window.owner_is_main {
            return Err(MacOSBackdropError::NotOnTheMainLane);
        }
        let main = MainThreadMarker::new()
            .expect("BUG: a main-lane window lives on the main thread, where it is used");
        if window.closed.load(Ordering::Acquire) {
            return Err(MacOSBackdropError::CLOSED);
        }
        install(window, main, backdrop)
    }

    /// Take the backdrop away: FLUI's view is the window's content view again.
    /// A window without a backdrop is left as it is.
    ///
    /// # Errors
    /// [`MacOSBackdropError`]: the window is closed or off the AppKit main
    /// lane, or its content view is one something else put there.
    pub fn clear_backdrop(self) -> Result<(), MacOSBackdropError> {
        let window = self.0;
        if !window.owner_is_main {
            return Err(MacOSBackdropError::NotOnTheMainLane);
        }
        let main = MainThreadMarker::new()
            .expect("BUG: a main-lane window lives on the main thread, where it is used");
        if window.closed.load(Ordering::Acquire) {
            return Err(MacOSBackdropError::CLOSED);
        }
        let content = content_of(window, main)?;
        take_away(window, main, content);
        Ok(())
    }
}

/// The class `name` names in the Objective-C runtime, if this system has it.
fn class_named(name: &std::ffi::CStr) -> Option<&'static AnyClass> {
    AnyClass::get(name)
}

/// Whether `object` is an instance of `class` or of a subclass.
fn is_kind(object: ObjcId, class: &AnyClass, _main: MainThreadMarker) -> bool {
    // SAFETY: `object` is a live view of the window, read on the main thread
    // (`_main`); `isKindOfClass:` is `NSObject`'s.
    let kind: BObjC = unsafe { msg_send![object, isKindOfClass: class] };
    kind.as_bool()
}

/// What the window's content view is, or why it is none of FLUI's.
fn content_of(window: &MacOSWindow, main: MainThreadMarker) -> Result<Content, MacOSBackdropError> {
    let view = window.view_ptr();
    // SAFETY: the wrapper owns a live NSWindow (released only after it is
    // closed, which the caller ruled out) and its FLUI view; read on the main
    // thread (`main`).
    let (current, superview): (ObjcId, ObjcId) = unsafe {
        (
            msg_send![window.ns_window, contentView],
            msg_send![view, superview],
        )
    };
    if current == view {
        return Ok(Content::Flui);
    }
    if let Some(effect) = class_named(c"NSVisualEffectView")
        && is_kind(current, effect, main)
        && superview == current
    {
        return Ok(Content::Material(current));
    }
    if let Some(glass) = class_named(c"NSGlassEffectView")
        && is_kind(current, glass, main)
    {
        // SAFETY: `current` is an `NSGlassEffectView` (checked above), whose
        // `contentView` is its documented property; on the main thread.
        let held: ObjcId = unsafe { msg_send![current, contentView] };
        if held == view {
            return Ok(Content::Glass(current));
        }
    }
    Err(MacOSBackdropError::ForeignContentView)
}

/// A new instance of `class`, `init`-ed, or `None` when AppKit made none.
fn new_view(class: &AnyClass, _main: MainThreadMarker) -> Option<Retained<AnyObject>> {
    // SAFETY: `alloc` then `init` is the designated way to make an `NSView`;
    // the +1 of `init` is taken over by the `Retained`, which `from_raw`
    // refuses for nil. On the main thread (`_main`), where views are made.
    unsafe {
        let allocated: ObjcId = msg_send![class, alloc];
        let initialized: ObjcId = msg_send![allocated, init];
        Retained::from_raw(initialized)
    }
}

/// Put `backdrop` behind the window's content.
fn install(
    window: &MacOSWindow,
    main: MainThreadMarker,
    backdrop: MacOSBackdrop,
) -> Result<(), MacOSBackdropError> {
    let current = content_of(window, main)?;
    match backdrop {
        MacOSBackdrop::Material(material) => {
            if let Content::Material(effect) = current {
                configure_material(effect, material, main);
                return Ok(());
            }
            let class =
                class_named(c"NSVisualEffectView").ok_or(MacOSBackdropError::NoBackdropView)?;
            let created = new_view(class, main).ok_or(MacOSBackdropError::NoBackdropView)?;
            let effect = Retained::as_ptr(&created).cast_mut();
            configure_material(effect, material, main);
            take_away(window, main, current);
            embed(window, main, effect, Holder::Subview);
            Ok(())
        }
        MacOSBackdrop::Glass(glass) => {
            let class = class_named(c"NSGlassEffectView").ok_or(MacOSBackdropError::NO_GLASS)?;
            let created = new_view(class, main).ok_or(MacOSBackdropError::NoBackdropView)?;
            let glass_view = Retained::as_ptr(&created).cast_mut();
            configure_glass(glass_view, glass, main);
            take_away(window, main, current);
            embed(window, main, glass_view, Holder::ContentView);
            Ok(())
        }
    }
}

/// How a backdrop view holds FLUI's view.
enum Holder {
    /// As a subview (`NSVisualEffectView`).
    Subview,
    /// As its `contentView` (`NSGlassEffectView`).
    ContentView,
}

/// Make `backdrop` the window's content view, holding FLUI's view.
fn embed(window: &MacOSWindow, _main: MainThreadMarker, backdrop: ObjcId, holder: Holder) {
    let view = window.view_ptr();
    // SAFETY: `backdrop` is a live view the caller holds a strong reference
    // to until this returns, and the window retains it as its content view;
    // the FLUI view is the wrapper's own, alive while it moves; the NSWindow
    // is live (not closed); all on the main thread (`_main`).
    unsafe {
        let frame: NSRect = msg_send![view, frame];
        let _: () = msg_send![backdrop, setFrame: frame];
        let _: () = msg_send![backdrop, setAutoresizingMask: FILL];
        let _: () = msg_send![window.ns_window, setContentView: backdrop];
        match holder {
            Holder::Subview => {
                let bounds: NSRect = msg_send![backdrop, bounds];
                let _: () = msg_send![view, setFrame: bounds];
                let _: () = msg_send![view, setAutoresizingMask: FILL];
                let _: () = msg_send![backdrop, addSubview: view];
            }
            Holder::ContentView => {
                let _: () = msg_send![view, setAutoresizingMask: FILL];
                let _: () = msg_send![backdrop, setContentView: view];
            }
        }
        // Leaving the window resigned the view's first-responder status;
        // keyboard and input-method input must keep reaching it.
        let _: BObjC = msg_send![window.ns_window, makeFirstResponder: view];
    }
}

/// Make FLUI's view the window's content view again, out of the backdrop
/// `content` holds it in.
fn take_away(window: &MacOSWindow, _main: MainThreadMarker, content: Content) {
    let view = window.view_ptr();
    if matches!(content, Content::Flui) {
        return;
    }
    // SAFETY: the backdrop view is the window's live content view, holding
    // the FLUI view, which the wrapper owns while it moves; the window
    // releases the backdrop view when FLUI's view replaces it; on the main
    // thread.
    unsafe {
        match content {
            Content::Material(_) => {
                let _: () = msg_send![view, removeFromSuperview];
            }
            Content::Glass(glass) => {
                let _: () = msg_send![glass, setContentView: NIL];
            }
            Content::Flui => {}
        }
        let _: () = msg_send![window.ns_window, setContentView: view];
        let _: BObjC = msg_send![window.ns_window, makeFirstResponder: view];
    }
}

fn configure_material(effect: ObjcId, material: MacOSMaterial, _main: MainThreadMarker) {
    // SAFETY: `effect` is a live `NSVisualEffectView`; each value is the
    // `NSInteger` enum type the setter takes; on the main thread.
    unsafe {
        let _: () = msg_send![effect, setMaterial: material.material()];
        let _: () = msg_send![effect, setBlendingMode: NSVisualEffectBlendingMode::BehindWindow];
        let _: () = msg_send![effect, setState: NSVisualEffectState::Active];
    }
}

fn configure_glass(glass: ObjcId, input: LiquidGlass, _main: MainThreadMarker) {
    // SAFETY: `glass` is a live `NSGlassEffectView`: `style` takes an
    // `NSGlassEffectViewStyle` (`NSInteger`), `cornerRadius` a `CGFloat`
    // (`f64`) and `tintColor` an `NSColor`, which the `Retained` keeps alive
    // through the call; on the main thread, inside an autorelease pool for
    // the colour's autoreleased reference.
    autoreleasepool(|_| unsafe {
        let _: () = msg_send![glass, setStyle: input.style.value()];
        if let Some(radius) = input.corner_radius {
            let _: () = msg_send![glass, setCornerRadius: radius];
        }
        if let Some(tint) = input.tint {
            let color: Option<Retained<AnyObject>> = msg_send![
                class!(NSColor),
                colorWithSRGBRed: tint.red,
                green: tint.green,
                blue: tint.blue,
                alpha: tint.alpha
            ];
            let color =
                color.expect("BUG: NSColor makes a colour of finite sRGB channels in 0..=1");
            let _: () = msg_send![glass, setTintColor: &*color];
        }
    });
}
