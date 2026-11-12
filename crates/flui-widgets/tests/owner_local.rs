//! A localization delegate, an image provider and an animated list's keys hold
//! owner-thread state: they carry no thread bound (ADR-0323 §4), so `Rc` and
//! `Cell` values pass.

use std::{cell::Cell, rc::Rc};

use flui_painting::paint::Image as PixelImage;
use flui_platform::Locale;
use flui_view::ViewExt;
use flui_widgets::{
    AnimatedList, BoxedLocalizationsDelegate, DefaultWidgetsLocalizationsDelegate, Image,
    ImageCacheKey, ImageProvider, ImageProviderError, Localizations, LocalizationsDelegate,
    SizedBox,
};

/// Resources that share owner-local state with their delegate.
#[derive(Debug)]
struct Greeting {
    _loads: Rc<Cell<u32>>,
}

#[derive(Debug)]
struct GreetingDelegate {
    loads: Rc<Cell<u32>>,
}

impl LocalizationsDelegate for GreetingDelegate {
    type Resources = Greeting;

    fn is_supported(&self, _: &Locale) -> bool {
        true
    }

    fn load(&self, _: &Locale) -> Greeting {
        self.loads.set(self.loads.get() + 1);
        Greeting {
            _loads: Rc::clone(&self.loads),
        }
    }
}

/// A provider that counts its synchronous resolves in owner-local state.
#[derive(Debug)]
struct CountedProvider(Rc<Cell<u32>>);

impl ImageProvider for CountedProvider {
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        self.0.set(self.0.get() + 1);
        Ok(PixelImage::from_rgba8(1, 1, vec![255; 4]))
    }

    fn cache_key(&self) -> Option<ImageCacheKey> {
        Some(ImageCacheKey::custom(Rc::new(7_u32)))
    }
}

#[test]
fn owner_local_values_build_localizations_images_and_animated_lists() {
    let loads = Rc::new(Cell::new(0));
    let delegate = BoxedLocalizationsDelegate::new(GreetingDelegate {
        loads: Rc::clone(&loads),
    });
    let locale = Locale::try_new("en", None::<&str>).expect("valid locale");
    let _localized = Localizations::new(
        locale,
        vec![
            delegate,
            BoxedLocalizationsDelegate::new(DefaultWidgetsLocalizationsDelegate),
        ],
        SizedBox::new(1.0, 1.0),
    )
    .boxed();

    let resolves = Rc::new(Cell::new(0));
    let _image = Image::new(CountedProvider(Rc::clone(&resolves))).boxed();

    let ids = [Rc::new(1_u32), Rc::new(2_u32)];
    let _list = AnimatedList::new(ids, Rc::clone, |_| SizedBox::new(1.0, 1.0)).boxed();

    assert_eq!(
        (loads.get(), resolves.get()),
        (0, 0),
        "building the views loads and resolves nothing"
    );
}
