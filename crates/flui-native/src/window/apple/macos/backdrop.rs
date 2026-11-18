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
use objc2::runtime::AnyClass;
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, Message, msg_send};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSColor, NSGlassEffectView, NSGlassEffectViewStyle, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
};

use super::MacOSWindow;
use super::ext::AppKitWindow;

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
    /// The `NSGlassEffectViewStyle` this is.
    fn style(self) -> NSGlassEffectViewStyle {
        match self {
            Self::Regular => NSGlassEffectViewStyle::Regular,
            Self::Clear => NSGlassEffectViewStyle::Clear,
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
    Material(Retained<NSVisualEffectView>),
    /// An `NSGlassEffectView` holding FLUI's view.
    Glass(Retained<NSGlassEffectView>),
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
    /// [`MacOSBackdropError`]: the window is closed,
    /// Liquid Glass before macOS 26, a corner radius that is not finite
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
        if window.closed.load(Ordering::Acquire) {
            return Err(MacOSBackdropError::CLOSED);
        }
        install(window, backdrop)
    }

    /// Take the backdrop away: FLUI's view is the window's content view again.
    /// A window without a backdrop is left as it is.
    ///
    /// # Errors
    /// [`MacOSBackdropError`]: the window is closed,
    /// or its content view is one something else put there.
    pub fn clear_backdrop(self) -> Result<(), MacOSBackdropError> {
        let window = self.0;
        if window.closed.load(Ordering::Acquire) {
            return Err(MacOSBackdropError::CLOSED);
        }
        let content = content_of(window)?;
        take_away(window, content);
        Ok(())
    }
}

/// The class `name` names in the Objective-C runtime, if this system has it.
fn class_named(name: &std::ffi::CStr) -> Option<&'static AnyClass> {
    AnyClass::get(name)
}

/// Whether `view` and `other` are one object.
fn is_same(view: &NSView, other: &NSView) -> bool {
    std::ptr::eq(view, other)
}

/// What the window's content view is, or why it is none of FLUI's.
fn content_of(window: &MacOSWindow) -> Result<Content, MacOSBackdropError> {
    let view: &NSView = &window.view;
    let Some(current) = window.ns_window.contentView() else {
        return Err(MacOSBackdropError::ForeignContentView);
    };
    if is_same(&current, view) {
        return Ok(Content::Flui);
    }
    if let Some(effect) = current.downcast_ref::<NSVisualEffectView>() {
        // SAFETY: `superview` is not retained by the view's own answer, so it
        // must still be alive when the call retains it: FLUI's view is the
        // wrapper's own and the window's content view holds what contains it,
        // and nothing between the two reads reorders the hierarchy, on the
        // main thread.
        let superview = unsafe { view.superview() };
        if superview.is_some_and(|superview| is_same(&superview, &current)) {
            return Ok(Content::Material(effect.retain()));
        }
    }
    // `NSGlassEffectView` is looked up before it is named: a system without
    // it has no such class to compare against.
    if class_named(c"NSGlassEffectView").is_some()
        && let Some(glass) = current.downcast_ref::<NSGlassEffectView>()
        && glass.contentView().is_some_and(|held| is_same(&held, view))
    {
        return Ok(Content::Glass(glass.retain()));
    }
    Err(MacOSBackdropError::ForeignContentView)
}

/// A new instance of `V`, `init`-ed, or `None` when AppKit made none.
fn new_view<V: ClassType + MainThreadOnly>(main: MainThreadMarker) -> Option<Retained<V>> {
    // SAFETY: `init` is the designated initializer of an `NSView`, sent once
    // to the instance just allocated, on the main thread `main` proves.
    unsafe { msg_send![V::alloc(main), init] }
}

/// Put `backdrop` behind the window's content.
fn install(window: &MacOSWindow, backdrop: MacOSBackdrop) -> Result<(), MacOSBackdropError> {
    let current = content_of(window)?;
    match backdrop {
        MacOSBackdrop::Material(material) => {
            if let Content::Material(effect) = &current {
                configure_material(effect, material);
                return Ok(());
            }
            let effect = new_view::<NSVisualEffectView>(window.main)
                .ok_or(MacOSBackdropError::NoBackdropView)?;
            configure_material(&effect, material);
            take_away(window, current);
            embed(window, BackdropView::Material(&effect));
            Ok(())
        }
        MacOSBackdrop::Glass(glass) => {
            if class_named(c"NSGlassEffectView").is_none() {
                return Err(MacOSBackdropError::NO_GLASS);
            }
            let glass_view = new_view::<NSGlassEffectView>(window.main)
                .ok_or(MacOSBackdropError::NoBackdropView)?;
            configure_glass(&glass_view, glass);
            take_away(window, current);
            embed(window, BackdropView::Glass(&glass_view));
            Ok(())
        }
    }
}

/// A backdrop view made here, and so how it holds FLUI's view.
enum BackdropView<'a> {
    /// As a subview (`NSVisualEffectView`).
    Material(&'a NSVisualEffectView),
    /// As its `contentView` (`NSGlassEffectView`).
    Glass(&'a NSGlassEffectView),
}

/// Make `backdrop` the window's content view, holding FLUI's view.
fn embed(window: &MacOSWindow, backdrop: BackdropView<'_>) {
    let view: &NSView = &window.view;
    let backdrop_view: &NSView = match backdrop {
        BackdropView::Material(effect) => effect,
        BackdropView::Glass(glass) => glass,
    };
    backdrop_view.setFrame(view.frame());
    backdrop_view.setAutoresizingMask(FILL);
    window.ns_window.setContentView(Some(backdrop_view));
    match backdrop {
        BackdropView::Material(effect) => {
            view.setFrame(effect.bounds());
            view.setAutoresizingMask(FILL);
            effect.addSubview(view);
        }
        BackdropView::Glass(glass) => {
            view.setAutoresizingMask(FILL);
            glass.setContentView(Some(view));
        }
    }
    // Leaving the window resigned the view's first-responder status;
    // keyboard and input-method input must keep reaching it.
    let _ = window.ns_window.makeFirstResponder(Some(view));
}

/// Make FLUI's view the window's content view again, out of the backdrop
/// `content` holds it in.
fn take_away(window: &MacOSWindow, content: Content) {
    let view: &NSView = &window.view;
    match content {
        Content::Flui => return,
        Content::Material(_) => view.removeFromSuperview(),
        Content::Glass(glass) => glass.setContentView(None),
    }
    // The window releases the backdrop view when FLUI's view replaces it.
    window.ns_window.setContentView(Some(view));
    let _ = window.ns_window.makeFirstResponder(Some(view));
}

fn configure_material(effect: &NSVisualEffectView, material: MacOSMaterial) {
    effect.setMaterial(material.material());
    effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect.setState(NSVisualEffectState::Active);
}

fn configure_glass(glass: &NSGlassEffectView, input: LiquidGlass) {
    // Inside an autorelease pool for the colour's autoreleased reference.
    autoreleasepool(|_| {
        glass.setStyle(input.style.style());
        if let Some(radius) = input.corner_radius {
            glass.setCornerRadius(radius);
        }
        if let Some(tint) = input.tint {
            let color = NSColor::colorWithSRGBRed_green_blue_alpha(
                tint.red, tint.green, tint.blue, tint.alpha,
            );
            glass.setTintColor(Some(&color));
        }
    });
}
