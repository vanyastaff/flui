//! The image cache a headless host's UI runtime keeps, as a widget acquires it.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_assets::{ImageCache, ImageCacheCapability};
use flui_platform::assets::{AssetBundle, AssetBundleCapability};
use flui_testing::HeadlessHost;
use flui_view::{
    BuildContext, IntoView, LifecycleContext, LifecycleContextExt as _, StatefulView, View,
    ViewState,
};

type Acquired = Rc<RefCell<Option<Arc<ImageCache>>>>;

/// Mount a widget that acquires [`ImageCacheCapability`] in a new window of
/// `host` and return the cache it was handed: the one every `Image` of that UI
/// runtime loads through.
pub fn runtime_image_cache(host: &mut HeadlessHost) -> Arc<ImageCache> {
    let acquired = Acquired::default();
    let window = host.open_window(flui_testing::HeadlessWindow::new(10, 10));
    host.attach_to(
        window,
        &CacheAcquirer {
            acquired: Rc::clone(&acquired),
        },
    )
    .expect("root attaches");
    let _ = host.pump(Duration::ZERO);
    acquired.take().expect("init_state ran")
}

#[derive(Clone)]
struct CacheAcquirer {
    acquired: Acquired,
}

struct CacheAcquirerState {
    acquired: Acquired,
}

impl StatefulView for CacheAcquirer {
    type State = CacheAcquirerState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        CacheAcquirerState {
            acquired: Rc::clone(&self.acquired),
        }
    }
}

impl View for CacheAcquirer {
    fn create_element(self: Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl ViewState<CacheAcquirer> for CacheAcquirerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let cache = ctx
            .capability::<ImageCacheCapability>()
            .expect("the headless host keeps an image cache per UI runtime");
        self.acquired.replace(Some(cache));
    }

    fn build(&self, _view: &CacheAcquirer, _ctx: &dyn BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::new(10.0, 10.0)
    }
}

type AcquiredBundle = Rc<RefCell<Option<AssetBundle>>>;

/// Mount a widget that acquires [`AssetBundleCapability`] in the primary
/// window of `host` and then shows `image`, and return the bundle it was
/// handed: the one every `Image::asset` of that UI runtime reads through, and
/// so the identity of its assets' cache keys.
#[allow(dead_code, reason = "used by the asset image tests only")]
pub fn runtime_asset_bundle(host: &mut HeadlessHost, image: flui_widgets::Image) -> AssetBundle {
    let acquired = AcquiredBundle::default();
    host.attach(&BundleAcquirer {
        acquired: Rc::clone(&acquired),
        image,
    })
    .expect("root attaches");
    let _ = host.pump(Duration::ZERO);
    acquired.take().expect("init_state ran")
}

#[derive(Clone)]
struct BundleAcquirer {
    acquired: AcquiredBundle,
    image: flui_widgets::Image,
}

struct BundleAcquirerState {
    acquired: AcquiredBundle,
}

impl StatefulView for BundleAcquirer {
    type State = BundleAcquirerState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        BundleAcquirerState {
            acquired: Rc::clone(&self.acquired),
        }
    }
}

impl View for BundleAcquirer {
    fn create_element(self: Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl ViewState<BundleAcquirer> for BundleAcquirerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let bundle = ctx
            .capability::<AssetBundleCapability>()
            .expect("the headless host keeps an asset bundle per window");
        self.acquired.replace(Some(bundle));
    }

    fn build(&self, view: &BundleAcquirer, _ctx: &dyn BuildContext) -> impl IntoView {
        view.image.clone()
    }
}
