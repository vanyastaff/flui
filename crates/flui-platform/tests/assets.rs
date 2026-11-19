//! The asset bundle's contract (ADR-0384 §2, §5): what every source answers
//! for a key it holds, one it does not, and one over the read's limit.

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use flui_platform::assets::{AssetBundle, AssetKey, AssetLoadError, AssetSource, DirectoryAssets};
use flui_platform::testing::MemoryAssets;

/// Polls `future` to its end on this thread: a source needs no runtime.
fn block_on<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let mut ctx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut ctx) {
            return value;
        }
    }
}

fn key(key: &str) -> AssetKey {
    AssetKey::try_new(key).expect("BUG: the test names a valid key")
}

/// The same answers from every source: the bytes of a key it holds, under
/// any limit at least their length; `NotFound` for a key it does not;
/// `TooLarge` for one over the limit; `NotUtf8` from `load_string` for bytes
/// that are not text. The directory reader adds `NotFound` for a key whose
/// path crosses a file, and `NotAFile` for a directory.
///
/// Red-check: let `DirectoryAssets` answer a missing file with `Io`, or
/// compare the limit with `>=`.
#[test]
fn every_asset_source_keeps_the_bundle_contract() {
    let root = std::env::temp_dir().join(format!("flui-assets-contract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("assets/images")).expect("BUG: the temp dir is writable");
    std::fs::write(root.join("assets/images/logo.png"), b"logo bytes").expect("BUG: writable");
    std::fs::write(root.join("assets/notes.txt"), [0xff, 0xfe]).expect("BUG: writable");

    let memory = MemoryAssets::new()
        .asset(key("assets/images/logo.png"), b"logo bytes".as_slice())
        .asset(key("assets/notes.txt"), [0xff_u8, 0xfe].as_slice());
    let sources: [(&str, Arc<dyn AssetSource>); 2] = [
        ("directory", Arc::new(DirectoryAssets::new(&root))),
        ("memory", Arc::new(memory)),
    ];

    for (name, source) in sources {
        let bundle = AssetBundle::new(source);
        let logo = key("assets/images/logo.png");
        assert_eq!(
            block_on(bundle.load(&logo, 10)).expect("a held key under its length reads"),
            b"logo bytes",
            "{name}"
        );
        assert_eq!(
            block_on(bundle.load_string(&logo, 1024)).expect("UTF-8 reads as text"),
            "logo bytes",
            "{name}"
        );
        assert!(
            matches!(
                block_on(bundle.load(&logo, 9)),
                Err(AssetLoadError::TooLarge { limit: 9, .. })
            ),
            "{name}: one byte over the limit"
        );
        let missing = key("assets/images/missing.png");
        let Err(error) = block_on(bundle.load(&missing, 1024)) else {
            panic!("{name}: a missing key has no bytes");
        };
        assert!(
            matches!(error, AssetLoadError::NotFound { .. }),
            "{name}: {error:?}"
        );
        assert_eq!(error.key(), &missing);
        assert!(
            matches!(
                block_on(bundle.load_string(&key("assets/notes.txt"), 1024)),
                Err(AssetLoadError::NotUtf8 { .. })
            ),
            "{name}"
        );
        assert!(
            matches!(
                block_on(bundle.load(&key("assets/images/logo.png/inner"), 1024)),
                Err(AssetLoadError::NotFound { .. })
            ),
            "{name}: a path through a file holds nothing"
        );
    }

    let directory = AssetBundle::new(Arc::new(DirectoryAssets::new(&root)));
    assert!(matches!(
        block_on(directory.load(&key("assets/images"), 1024)),
        Err(AssetLoadError::NotAFile { .. })
    ));
    let _ = std::fs::remove_dir_all(&root);
}

/// Clones of a bundle share its authority, two sources never do, and a
/// dropped source's authority matches no later one.
///
/// Red-check: compare authorities by `Weak::upgrade`, or derive one from a
/// clone's own allocation.
#[test]
fn a_bundles_authority_is_its_source() {
    let bundle = AssetBundle::new(Arc::new(DirectoryAssets::new("root")));
    let clone = bundle.clone();
    assert_eq!(bundle.authority(), clone.authority());
    drop(clone);
    assert_ne!(
        bundle.authority(),
        AssetBundle::new(Arc::new(DirectoryAssets::new("root"))).authority()
    );
    let dead = bundle.authority();
    drop(bundle);
    for _ in 0..1024 {
        assert_ne!(
            dead,
            AssetBundle::new(Arc::new(DirectoryAssets::new("root"))).authority()
        );
    }
}
