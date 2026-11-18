//! The drag position's vocabulary: how much of a source's types every host
//! reads. It lives beside the table, not in the `drag` feature, because the
//! transfer objects the clipboard and drag share (the Wayland seats' data
//! offers) read a source's types too, and they name no feature.

/// The most types an offer is read from. A drag's source (and, on Wayland, a
/// selection's) is another application, which names its types in its own
/// order and as many as it likes; every host reads the first
/// `MAX_OFFERED_TYPES` of them and stops there, whether the types come as a
/// list (`XdndTypeList`, a format enumerator, `ClipDescription`,
/// `DataTransfer.types`) or one event at a time (`wl_data_offer.offer`, where
/// a repeated name counts too). Far more than any toolkit offers.
#[cfg(any(
    windows,
    target_os = "android",
    target_arch = "wasm32",
    x11_host,
    wayland_host,
    test
))]
pub(crate) const MAX_OFFERED_TYPES: usize = 100;

/// The longest type name read, in bytes. A media type's type and subtype are
/// at most 127 characters each (RFC 6838 §4.2), and an X atom's or a Win32
/// clipboard format's name at most 255, so a longer name names nothing a host
/// offers: among the first [`MAX_OFFERED_TYPES`], it is left out before it is
/// copied, and it counts toward them.
#[cfg(any(
    windows,
    target_os = "android",
    target_arch = "wasm32",
    x11_host,
    wayland_host,
    test
))]
pub(crate) const MAX_TYPE_LEN: usize = 255;
