use std::sync::Arc;
use std::time::{Duration, Instant};

use flui::assets::{AssetRegistry, AssetRegistryBuilder, ImageLoad, ImageSource, LoadServices};
use flui::testing::widgets::{lay_out, loose, size};
use flui::widgets::Image;

fn start_asset_load(registry: &AssetRegistry, services: LoadServices) -> ImageLoad {
    registry.load_image("src/tiny.png", services)
}

#[test]
fn asset_image_loads_through_facade() {
    let registry = Arc::new(AssetRegistryBuilder::new().build());
    let asset = Image::asset(Arc::clone(&registry), "src/tiny.png");
    let _network = Image::network(
        Arc::clone(&registry),
        "https://example.test/fixture.png",
    );
    let source = ImageSource::Asset(Arc::from("src/tiny.png"));
    assert!(registry.cached_image(&source).is_none());
    let _cold_load = start_asset_load(&registry, LoadServices::new());

    let mut laid = lay_out(asset, loose(100.0));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        laid.tick();
        if laid.size(laid.current_root()) == size(5.0, 3.0) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the facade asset image did not decode within five seconds"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let decoded = registry
        .cached_image(&source)
        .expect("the completed image is in the application-owned registry");
    assert_eq!((decoded.width(), decoded.height()), (5, 3));
}
