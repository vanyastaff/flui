//! The asset key's allow-list (ADR-0384 §1): which keys exist.

use flui_platform::assets::{AssetKey, AssetKeyError, MAX_ASSET_KEY_LEN};

/// Every key that could name a path outside its bundle's root, or a file
/// Windows names differently, on any
/// platform, is refused before a reader sees it; the keys a project
/// declares pass as written.
///
/// Red-check: drop one arm of `AssetKey::try_new`'s allow-list.
#[test]
fn asset_key_refuses_every_key_outside_its_bundle() {
    let refused: [(&str, AssetKeyError); 27] = [
        ("assets/CONIN$", AssetKeyError::ReservedSegment),
        ("assets/nul .txt", AssetKeyError::ReservedSegment),
        ("assets/COM¹.png", AssetKeyError::ReservedSegment),
        (
            "assets/logo?.png",
            AssetKeyError::Forbidden { character: '?' },
        ),
        ("assets/*.png", AssetKeyError::Forbidden { character: '*' }),
        (
            "assets/\"logo\".png",
            AssetKeyError::Forbidden { character: '"' },
        ),
        (
            "assets/<logo>.png",
            AssetKeyError::Forbidden { character: '<' },
        ),
        (
            "assets/a|b.png",
            AssetKeyError::Forbidden { character: '|' },
        ),
        ("assets/NUL", AssetKeyError::ReservedSegment),
        ("assets/con.txt", AssetKeyError::ReservedSegment),
        ("Com1/logo.png", AssetKeyError::ReservedSegment),
        ("assets/lpt9.tar.gz", AssetKeyError::ReservedSegment),
        ("assets/logo.png.", AssetKeyError::ReservedSegment),
        ("assets /logo.png", AssetKeyError::ReservedSegment),
        ("", AssetKeyError::Empty),
        ("/etc/passwd", AssetKeyError::EmptySegment),
        ("assets/", AssetKeyError::EmptySegment),
        ("assets//logo.png", AssetKeyError::EmptySegment),
        ("..", AssetKeyError::DotSegment),
        ("assets/../../secret", AssetKeyError::DotSegment),
        ("./assets/logo.png", AssetKeyError::DotSegment),
        (
            "assets\\..\\secret",
            AssetKeyError::Forbidden { character: '\\' },
        ),
        (
            "C:/Windows/win.ini",
            AssetKeyError::Forbidden { character: ':' },
        ),
        (
            "assets/logo.png:stream",
            AssetKeyError::Forbidden { character: ':' },
        ),
        (
            "assets/logo\0.png",
            AssetKeyError::Forbidden { character: '\0' },
        ),
        (
            "assets/logo\n.png",
            AssetKeyError::Forbidden { character: '\n' },
        ),
        (
            "assets/logo\u{7f}.png",
            AssetKeyError::Forbidden {
                character: '\u{7f}',
            },
        ),
    ];
    for (key, error) in refused {
        assert_eq!(AssetKey::try_new(key), Err(error), "{key:?}");
    }
    let long = "a".repeat(MAX_ASSET_KEY_LEN + 1);
    assert_eq!(
        AssetKey::try_new(&long),
        Err(AssetKeyError::TooLong {
            len: MAX_ASSET_KEY_LEN + 1
        })
    );
    assert!(
        AssetKey::try_new(&long[1..]).is_ok(),
        "the limit itself is a key"
    );

    for admitted in [
        "logo.png",
        "assets/images/logo.png",
        "assets/.hidden",
        "assets/a..b",
        "assets/фото 1.png",
        "assets/console.png",
        "assets/com10.png",
        "assets/#1 %20.png",
    ] {
        let admitted_key = AssetKey::try_new(admitted).expect("an admitted key");
        assert_eq!(admitted_key.as_str(), admitted);
        assert_eq!(
            admitted_key.segments().collect::<Vec<_>>().join("/"),
            admitted
        );
    }
}
