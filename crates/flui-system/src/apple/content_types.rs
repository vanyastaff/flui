//! What a dialog's filters ask an Apple system to allow, and the contract's
//! rule its picks are held to (ADR-0360 §3, §4).
//!
//! macOS's panels and iOS's document picker filter by content type: an
//! extension becomes the type the system derives from its last part
//! (`UTType typeWithFilenameExtension:`, which also finds a type the
//! application's bundle exports or imports, ADR-0361), a MIME type its
//! type, and `image/*`, `audio/*`, `video/*`, `text/*` and `font/*` the
//! family's type (`public.image`, …); another `type/*` names no family the
//! systems have and is refused with `NotOnThisPlatform` rather than
//! widened.
//!
//! The systems admit a file whose type conforms to an allowed one (`.swift`
//! under `txt`, `.docx` under `zip`) and derive a type from the last part
//! of a name, while the contract matches names exactly. So an item is
//! judged again by the contract's rule ([`judge_item`]): by its name first,
//! which reads nothing of the file, then by its type against the MIME
//! patterns' types, and only a type that was read refuses it.
//!
//! `UTType` is macOS 11 and iOS 14, in a framework the floors lack, so the
//! class is looked up at run time behind `HasContentTypes`, never linked.

use flui_platform::{Unsupported, UnsupportedReason};

use crate::dialog::{DialogError, FileDialogCapability, FileFilter, name_ends_in};

/// What a filter pattern asks the system to allow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ContentType {
    /// The type of files whose name ends in `.` and this, the last part of
    /// an extension.
    Extension(String),
    /// The type of this MIME type.
    Mime(String),
    /// The system type of this identifier: a MIME type family.
    Identifier(&'static str),
}

/// What a request's filters ask of the system's dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ContentFilter {
    /// The content types the dialog allows, each once.
    pub(crate) types: Vec<ContentType>,
    /// Every extension of the request, for the name check.
    pub(crate) extensions: Vec<String>,
}

impl ContentFilter {
    /// The file types a macOS panel without content types allows: the
    /// extensions' last parts, or `None` when a MIME pattern needs a type.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn file_types(&self) -> Option<Vec<String>> {
        self.types
            .iter()
            .map(|content_type| match content_type {
                ContentType::Extension(extension) => Some(extension.clone()),
                ContentType::Mime(_) | ContentType::Identifier(_) => None,
            })
            .collect::<Option<Vec<String>>>()
    }
}

/// The refusal of a request the dialog cannot show, for `reason`.
pub(crate) fn unsupported(reason: UnsupportedReason) -> DialogError {
    DialogError::Unsupported(Unsupported::of::<FileDialogCapability>(reason))
}

/// The content type a MIME type pattern is: the type itself, or a family
/// the systems have a type for.
fn mime_content_type(mime_type: &str) -> Result<ContentType, DialogError> {
    match mime_type.split_once('/') {
        Some((family, "*")) => {
            let identifier = match family {
                "image" => "public.image",
                "audio" => "public.audio",
                "video" => "public.movie",
                "text" => "public.text",
                "font" => "public.font",
                _ => return Err(unsupported(UnsupportedReason::NotOnThisPlatform)),
            };
            Ok(ContentType::Identifier(identifier))
        }
        _ => Ok(ContentType::Mime(mime_type.to_owned())),
    }
}

/// What `filters` ask of the system's dialog; `None` when there are none,
/// and the dialog allows every file.
pub(crate) fn content_filter(filters: &[FileFilter]) -> Result<Option<ContentFilter>, DialogError> {
    if filters.is_empty() {
        return Ok(None);
    }
    let mut filter = ContentFilter {
        types: Vec::new(),
        extensions: Vec::new(),
    };
    let allow = |content_type: ContentType, types: &mut Vec<ContentType>| {
        if !types.contains(&content_type) {
            types.push(content_type);
        }
    };
    for requested in filters {
        for extension in requested.extensions() {
            let (_, last) = extension.rsplit_once('.').unwrap_or(("", extension));
            allow(ContentType::Extension(last.to_owned()), &mut filter.types);
            if !filter.extensions.contains(extension) {
                filter.extensions.push(extension.clone());
            }
        }
        for mime_type in requested.mime_types() {
            allow(mime_content_type(mime_type)?, &mut filter.types);
        }
    }
    Ok(Some(filter))
}

/// Whether a file named `name` is one `extensions` name: the contract's
/// rule for a name (ADR-0360 §3), the one the handle checks a pick by.
pub(crate) fn name_matches(name: &[u8], extensions: &[String]) -> bool {
    extensions
        .iter()
        .any(|extension| name_ends_in(name, extension))
}

/// What the contract's rule makes of one item a dialog shows or answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Judged {
    /// Its name ends in one of the extensions, or its type is one of the
    /// MIME patterns'.
    Admitted,
    /// Neither, and its type was read.
    Refused,
    /// Its name does not admit it, and its type could not be read.
    Unread,
}

/// Judge an item named `name` (`None` for one with no name, a root) by the
/// request's `extensions`, and by its type only when its name does not
/// admit it: `conforms` reads whether its type is one of the MIME patterns',
/// `None` when the file system does not say. A name that admits the item
/// reads nothing, so a file that cannot be read is never refused for it.
pub(crate) fn judge_item(
    name: Option<&[u8]>,
    extensions: &[String],
    conforms: impl FnOnce() -> Option<bool>,
) -> Judged {
    if name.is_some_and(|name| name_matches(name, extensions)) {
        return Judged::Admitted;
    }
    match conforms() {
        Some(true) => Judged::Admitted,
        Some(false) => Judged::Refused,
        None => Judged::Unread,
    }
}

/// `UTType` and the panels' content types, macOS 11: below it a filter is
/// the panel's allowed file types, and one with a MIME type answers
/// `NotOnThisDevice`. On arm64 the floor is 11, and the types are always
/// there.
#[cfg(all(any(target_os = "macos", test), not(target_arch = "aarch64")))]
pub(crate) enum ContentTypesOnMacOS {}

#[cfg(all(any(target_os = "macos", test), not(target_arch = "aarch64")))]
impl super::version::Gate for ContentTypesOnMacOS {
    type System = super::version::OnMacOS;
    const SINCE: (u32, u32, u32) = (11, 0, 0);
}

/// `UTType` and the document picker's initializers that take content
/// types, iOS 14: below it an open without filters shows the picker of
/// every document, and one with filters answers `NotOnThisDevice`.
#[cfg(any(target_os = "ios", test))]
pub(crate) enum ContentTypesOnIOS {}

#[cfg(any(target_os = "ios", test))]
impl super::version::Gate for ContentTypesOnIOS {
    type System = super::version::OnIOS;
    const SINCE: (u32, u32, u32) = (14, 0, 0);
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use foundation::{HasContentTypes, NameRule, ut_type};

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[expect(unsafe_code, reason = "UTType through the runtime")]
mod foundation {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::{NSString, NSURL};

    use super::super::url_path;
    use super::{ContentFilter, ContentType, Judged, judge_item};

    /// Evidence that the running system has `UTType` and its dialogs'
    /// content types; made from the system's version alone.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct HasContentTypes(());

    #[cfg(target_os = "macos")]
    impl HasContentTypes {
        /// The evidence on macOS at `version`.
        #[cfg(target_arch = "aarch64")]
        #[expect(
            clippy::unnecessary_wraps,
            reason = "the x86_64 build of the same function answers None below macOS 11"
        )]
        pub(crate) fn on(_version: super::super::version::MacOSVersion) -> Option<Self> {
            Some(Self(()))
        }

        /// The evidence on macOS at `version`.
        #[cfg(not(target_arch = "aarch64"))]
        pub(crate) fn on(version: super::super::version::MacOSVersion) -> Option<Self> {
            version
                .since::<super::ContentTypesOnMacOS>()
                .map(|_| Self(()))
        }
    }

    #[cfg(target_os = "ios")]
    impl HasContentTypes {
        /// The evidence on iOS at `version`.
        pub(crate) fn on(version: super::super::version::IOSVersion) -> Option<Self> {
            version
                .since::<super::ContentTypesOnIOS>()
                .map(|_| Self(()))
        }
    }

    /// The `UTType` `content_type` names, looked up at run time: the class
    /// is in UniformTypeIdentifiers, which AppKit and UIKit load on the
    /// systems the evidence is made on. `None` for a type the system does
    /// not know. The class answers on any thread.
    pub(crate) fn ut_type(
        _: HasContentTypes,
        content_type: &ContentType,
    ) -> Option<Retained<AnyObject>> {
        let class = AnyClass::get(c"UTType")?;
        // SAFETY: each is a class method of `UTType` taking an `NSString`
        // and returning a nullable, autoreleased `UTType`.
        unsafe {
            match content_type {
                ContentType::Extension(extension) => msg_send![
                    class,
                    typeWithFilenameExtension: &*NSString::from_str(extension)
                ],
                ContentType::Mime(mime_type) => {
                    msg_send![class, typeWithMIMEType: &*NSString::from_str(mime_type)]
                }
                ContentType::Identifier(identifier) => {
                    msg_send![class, typeWithIdentifier: &*NSString::from_str(identifier)]
                }
            }
        }
    }

    /// The contract's rule for the items of a filtered dialog: the
    /// request's extensions, matched by name, and the types of its MIME
    /// patterns (none without the evidence, where such a request is
    /// refused).
    pub(crate) struct NameRule {
        extensions: Vec<String>,
        mime_types: Vec<Retained<AnyObject>>,
    }

    impl NameRule {
        /// The rule of `filter`, on a system with `content_types` or
        /// without.
        pub(crate) fn new(content_types: Option<HasContentTypes>, filter: &ContentFilter) -> Self {
            let mime_types = content_types.map_or_else(Vec::new, |proof| {
                filter
                    .types
                    .iter()
                    .filter(|content_type| !matches!(content_type, ContentType::Extension(_)))
                    .filter_map(|content_type| ut_type(proof, content_type))
                    .collect::<Vec<Retained<AnyObject>>>()
            });
            Self {
                extensions: filter.extensions.clone(),
                mime_types,
            }
        }

        /// Whether the rule has a MIME pattern's type to judge by: without
        /// one a name alone answers, and the handle checks every pick's.
        #[cfg(target_os = "macos")]
        pub(crate) fn judges_types(&self) -> bool {
            !self.mime_types.is_empty()
        }

        /// What the rule makes of `item`: by its name first, then by
        /// `conforms`, which reads whether its type is one of the rule's
        /// (`None` when the file system does not say) and is not asked for
        /// an item its name admits.
        pub(crate) fn judge(
            &self,
            item: &NSURL,
            conforms: impl FnOnce() -> Option<bool>,
        ) -> Judged {
            use std::os::unix::ffi::OsStrExt as _;

            let path = url_path(item);
            let name = path
                .as_deref()
                .and_then(std::path::Path::file_name)
                .map(std::ffi::OsStr::as_bytes);
            judge_item(name, &self.extensions, conforms)
        }

        /// Whether a file of the type `identifier` is one of the rule's
        /// MIME types.
        pub(crate) fn conforms(&self, identifier: Option<&NSString>) -> bool {
            if self.mime_types.is_empty() {
                return false;
            }
            let (Some(identifier), Some(class)) = (identifier, AnyClass::get(c"UTType")) else {
                return false;
            };
            // SAFETY: a class method of `UTType` taking an `NSString`.
            let file_type: Option<Retained<AnyObject>> =
                unsafe { msg_send![class, typeWithIdentifier: identifier] };
            file_type.is_some_and(|file_type| {
                self.mime_types.iter().any(|allowed| {
                    // SAFETY: `conformsToType:` takes a `UTType` and answers
                    // a `BOOL`; both are `UTType`s.
                    unsafe { msg_send![&*file_type, conformsToType: &**allowed] }
                })
            })
        }

        /// Whether `item`'s type, read from the file system, is one of the
        /// rule's MIME types; `None` when the file system does not say (a
        /// file gone since it was picked, a volume that stopped answering).
        /// A rule without MIME types reads nothing.
        #[cfg(target_os = "ios")]
        #[expect(
            deprecated,
            reason = "its successor, NSURLContentTypeKey, is iOS 14 and a symbol the floor's Foundation lacks"
        )]
        pub(crate) fn type_conforms(&self, item: &NSURL) -> Option<bool> {
            if self.mime_types.is_empty() {
                return Some(false);
            }
            // SAFETY: Foundation's resource key constants live for the
            // process; this one is in every iOS FLUI supports.
            let type_key = unsafe { objc2_foundation::NSURLTypeIdentifierKey };
            let values = item
                .resourceValuesForKeys_error(&objc2_foundation::NSArray::from_slice(&[type_key]))
                .ok()?;
            let identifier = values
                .objectForKey(type_key)
                .and_then(|identifier| identifier.downcast::<NSString>().ok());
            Some(self.conforms(identifier.as_deref()))
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::{Unsupported, UnsupportedReason};

    use super::{ContentType, Judged, content_filter, judge_item, name_matches};
    use crate::dialog::{DialogError, FileDialogCapability, FileFilter};

    fn extensions(list: &[&str]) -> Vec<String> {
        list.iter()
            .map(|extension| (*extension).to_owned())
            .collect()
    }

    #[test]
    fn a_filter_allows_each_type_once_and_a_several_part_extension_by_its_last_part() {
        let filters = [
            FileFilter::new("Archives")
                .with_extension("tar.gz")
                .with_extension("zip"),
            FileFilter::new("Images")
                .with_extension("PNG")
                .with_extension("zip")
                .with_mime_type("image/*")
                .with_mime_type("image/webp"),
        ];
        let filter = content_filter(&filters)
            .expect("every pattern maps")
            .expect("filters were given");
        assert_eq!(
            filter.types,
            [
                ContentType::Extension("gz".to_owned()),
                ContentType::Extension("zip".to_owned()),
                ContentType::Extension("png".to_owned()),
                ContentType::Identifier("public.image"),
                ContentType::Mime("image/webp".to_owned()),
            ]
        );
        assert_eq!(filter.extensions, extensions(&["tar.gz", "zip", "png"]));
        assert_eq!(filter.file_types(), None, "a MIME pattern needs a type");

        let plain = content_filter(&[FileFilter::new("Archives").with_extension("tar.gz")])
            .expect("maps")
            .expect("given");
        assert_eq!(
            plain.file_types(),
            Some(extensions(&["gz"])),
            "below macOS 11 a panel allows the last part, and the delegate checks the name"
        );
        assert_eq!(content_filter(&[]), Ok(None), "no filter allows every file");
    }

    #[test]
    fn a_mime_family_the_systems_have_no_type_for_is_refused_rather_than_widened() {
        for family in ["image/*", "audio/*", "video/*", "text/*", "font/*"] {
            assert!(
                content_filter(&[FileFilter::new("F").with_mime_type(family)]).is_ok(),
                "{family}"
            );
        }
        assert_eq!(
            content_filter(&[FileFilter::new("Models").with_mime_type("model/*")]),
            Err(DialogError::Unsupported(Unsupported::of::<
                FileDialogCapability,
            >(
                UnsupportedReason::NotOnThisPlatform
            )))
        );
    }

    #[test]
    fn a_name_matches_as_the_contract_says() {
        let rule = extensions(&["tar.gz", "png", "txt", "zip"]);
        let cases: [(&[u8], bool); 13] = [
            (b"backup.tar.gz", true),
            (b"BACKUP.TAR.GZ", true),
            (b"photo.Png", true),
            (b"notes.gz", false),
            (b"tar.gz", false),
            // A name is longer than its extension and the dot before it.
            (b".png", false),
            (b"png", false),
            (b"photo.png.txt", true),
            ("é.png".as_bytes(), true),
            // A name that is not UTF-8 still ends in its extension.
            (b"\xff\xfe.png", true),
            // Types that conform to an allowed one are not its names.
            (b"main.swift", false),
            (b"photo.jpeg", false),
            (b"letter.docx", false),
        ];
        for (name, matches) in cases {
            assert_eq!(
                name_matches(name, &rule),
                matches,
                "{}",
                String::from_utf8_lossy(name)
            );
        }
    }

    #[test]
    fn an_items_name_admits_it_before_anything_of_it_is_read() {
        let rule = extensions(&["png", "tar.gz"]);
        assert_eq!(
            judge_item(Some(b"photo.PNG"), &rule, || panic!(
                "a name that admits its file reads nothing of it"
            )),
            Judged::Admitted
        );
        /// An item's name, what its type's read answers, and the verdict.
        type Case<'a> = (Option<&'a [u8]>, Option<bool>, Judged);
        let cases: [Case<'_>; 5] = [
            // A file whose type is one of the MIME patterns'.
            (Some(b"scan"), Some(true), Judged::Admitted),
            (Some(b"main.swift"), Some(false), Judged::Refused),
            // A file gone since it was listed, a volume that stopped
            // answering: not read, so not refused.
            (Some(b"scan"), None, Judged::Unread),
            (Some(b"notes.gz"), None, Judged::Unread),
            (None, Some(false), Judged::Refused),
        ];
        for (name, conforms, judged) in cases {
            assert_eq!(
                judge_item(name, &rule, || conforms),
                judged,
                "{name:?} {conforms:?}"
            );
        }
    }

    #[cfg(not(target_arch = "aarch64"))]
    #[test]
    fn content_types_are_there_from_macos_11_on() {
        use super::super::version::MacOSVersion;
        use super::ContentTypesOnMacOS;

        assert!(
            MacOSVersion::new(10, 15, 7)
                .since::<ContentTypesOnMacOS>()
                .is_none()
        );
        assert!(
            MacOSVersion::new(11, 0, 0)
                .since::<ContentTypesOnMacOS>()
                .is_some()
        );
    }

    #[test]
    fn content_types_are_there_from_ios_14_on() {
        use super::super::version::IOSVersion;
        use super::ContentTypesOnIOS;

        assert!(
            IOSVersion::new(13, 7, 0)
                .since::<ContentTypesOnIOS>()
                .is_none()
        );
        assert!(
            IOSVersion::new(14, 0, 0)
                .since::<ContentTypesOnIOS>()
                .is_some()
        );
    }
}
