//! The driver's pipeline cache, kept between runs in the application's
//! machine-local storage.
//!
//! A Vulkan driver compiles each pipeline into a `wgpu::PipelineCache` and
//! hands its data back on request. Desktop drivers keep a disk cache of their
//! own; Android's Vulkan drivers do not, and there every launch would compile
//! every pipeline again. [`PipelineStore::load`] seeds a context's cache with
//! what the last run saved, and [`PipelineStore::save`] writes it back: at the
//! end of the pipeline warm-up, off the frame path, and when the context
//! retires, for the pipelines built since.
//!
//! The value is one [`Storage`] name, [`PIPELINE_CACHE`], written with
//! [`WriteMode::Replace`]: the platform store stages each write in a file of
//! its own and renames it over the value (`flui_native::storage::FileStore`),
//! so a run killed mid-write leaves the previous value whole. What is stored
//! is an envelope ([`encode`]) that names the adapter the data is for and
//! hashes it, because `wgpu::Device::create_pipeline_cache` trusts its bytes:
//! wgpu checks only their header, and the driver reads the rest. Bytes the
//! envelope does not prove ([`decode`]) are never handed to wgpu; the value is
//! emptied instead, so it is not read again, and the context starts cold.

use std::sync::Arc;

use flui_platform::{Storage, StorageError, StorageName, WriteMode};
use sha2::{Digest, Sha256};

/// The stored value: this machine's compiled pipelines, which another
/// machine's driver cannot use.
pub(crate) const PIPELINE_CACHE: StorageName = StorageName::machine_local("gpu-pipeline-cache");

/// The longest envelope read or written, in bytes. A driver's cache for the
/// engine's pipelines is a few megabytes at most; anything longer is not one.
pub(crate) const MAX_ENVELOPE_BYTES: u64 = 64 * 1024 * 1024;

/// The first bytes of every envelope, and its format's version.
const TAG: [u8; 8] = *b"FLUIPLC1";

/// The bytes of the envelope's SHA-256 digest.
const DIGEST_BYTES: usize = 32;

/// Why stored bytes are not a pipeline cache for this adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Longer than [`MAX_ENVELOPE_BYTES`].
    TooLarge,
    /// Not an envelope of this format.
    Tag,
    /// An envelope for another adapter or driver family.
    OtherAdapter,
    /// Shorter than its header or than the payload it announces.
    Truncated,
    /// Longer than the payload it announces.
    Extended,
    /// The digest does not match the adapter key and the payload.
    Corrupt,
}

/// The envelope of `payload`, the data `wgpu::PipelineCache::get_data`
/// returned on an adapter whose `wgpu::util::pipeline_cache_key` is `key`:
///
/// | bytes | content |
/// |---|---|
/// | 8 | [`TAG`] |
/// | 4 | the key's length, `u32` little-endian |
/// | key's length | the key, UTF-8 |
/// | 8 | the payload's length, `u64` little-endian |
/// | 32 | SHA-256 of the key's length, the key and the payload |
/// | payload's length | the payload |
///
/// `None` when the envelope would be longer than [`MAX_ENVELOPE_BYTES`],
/// checked before anything is copied.
pub(crate) fn encode(key: &str, payload: &[u8]) -> Option<Vec<u8>> {
    let key_len = u32::try_from(key.len()).ok()?;
    let payload_len = u64::try_from(payload.len()).ok()?;
    let header = TAG.len() + 4 + key.len() + 8 + DIGEST_BYTES;
    let total = u64::try_from(header).ok()?.checked_add(payload_len)?;
    if total > MAX_ENVELOPE_BYTES {
        return None;
    }
    let mut envelope = Vec::with_capacity(usize::try_from(total).ok()?);
    envelope.extend_from_slice(&TAG);
    envelope.extend_from_slice(&key_len.to_le_bytes());
    envelope.extend_from_slice(key.as_bytes());
    envelope.extend_from_slice(&payload_len.to_le_bytes());
    envelope.extend_from_slice(&digest(key_len, key.as_bytes(), payload));
    envelope.extend_from_slice(payload);
    Some(envelope)
}

/// The payload `envelope` holds for the adapter whose cache key is `key`:
/// `Ok(None)` for an empty value, which is what a refused value is replaced
/// with, since a [`Storage`] cannot delete one.
///
/// # Errors
/// The [`Refusal`] of the first check `envelope` fails; its payload is never
/// returned then.
pub(crate) fn decode<'a>(key: &str, envelope: &'a [u8]) -> Result<Option<&'a [u8]>, Refusal> {
    if envelope.is_empty() {
        return Ok(None);
    }
    if u64::try_from(envelope.len()).map_or(true, |len| len > MAX_ENVELOPE_BYTES) {
        return Err(Refusal::TooLarge);
    }
    let mut rest = envelope;
    if take(&mut rest, TAG.len())? != TAG {
        return Err(Refusal::Tag);
    }
    let key_len = u32::from_le_bytes(array(take(&mut rest, 4)?));
    let stored_key = take(
        &mut rest,
        usize::try_from(key_len).map_err(|_| Refusal::Truncated)?,
    )?;
    if stored_key != key.as_bytes() {
        return Err(Refusal::OtherAdapter);
    }
    let payload_len = u64::from_le_bytes(array(take(&mut rest, 8)?));
    let stored_digest = take(&mut rest, DIGEST_BYTES)?;
    let held = u64::try_from(rest.len()).map_err(|_| Refusal::TooLarge)?;
    if held < payload_len {
        return Err(Refusal::Truncated);
    }
    if held > payload_len {
        return Err(Refusal::Extended);
    }
    if digest(key_len, stored_key, rest) != stored_digest {
        return Err(Refusal::Corrupt);
    }
    Ok(Some(rest))
}

/// The first `len` bytes of `rest`, which keeps what follows them.
fn take<'a>(rest: &mut &'a [u8], len: usize) -> Result<&'a [u8], Refusal> {
    if rest.len() < len {
        return Err(Refusal::Truncated);
    }
    let (head, tail) = rest.split_at(len);
    *rest = tail;
    Ok(head)
}

/// `bytes` as an array; [`take`] took exactly `N` of them.
fn array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    bytes
        .try_into()
        .expect("BUG: take returns exactly the length it was asked for")
}

fn digest(key_len: u32, key: &[u8], payload: &[u8]) -> [u8; DIGEST_BYTES] {
    let mut hasher = Sha256::new();
    hasher.update(key_len.to_le_bytes());
    hasher.update(key);
    hasher.update(payload);
    hasher.finalize().into()
}

/// A context's pipeline cache and the storage its data is saved in.
///
/// Cloned into the warm-up worker, which saves once its pipelines are built;
/// every clone names the same cache.
#[derive(Clone)]
pub(crate) struct PipelineStore {
    cache: wgpu::PipelineCache,
    /// The `wgpu::util::pipeline_cache_key` of the adapter the cache's
    /// device is on.
    key: Arc<str>,
    storage: Arc<dyn Storage>,
    /// Whether wgpu took the stored data the cache was created from.
    #[cfg(test)]
    seeded: bool,
}

impl PipelineStore {
    /// `device`'s pipeline cache, seeded with the data `storage` kept for
    /// `adapter`; `None` where the device has no pipeline cache (another
    /// backend than Vulkan, or a device without
    /// [`wgpu::Features::PIPELINE_CACHE`]).
    ///
    /// Never fails: a read error leaves the cache cold, and a value the
    /// envelope refuses is emptied as well, so the next run does not read it
    /// again.
    pub(crate) async fn load(
        device: &wgpu::Device,
        adapter: &wgpu::AdapterInfo,
        storage: Arc<dyn Storage>,
    ) -> Option<Self> {
        let key = cache_key(device, adapter)?;
        let held = match storage.read(&PIPELINE_CACHE, MAX_ENVELOPE_BYTES).await {
            Ok(stored) => stored.bytes,
            Err(StorageError::TooLarge { len, .. }) => {
                tracing::warn!(len, "the stored pipeline cache is too large; starting cold");
                empty(&*storage);
                None
            }
            Err(error) => {
                tracing::debug!(%error, "the stored pipeline cache cannot be read; starting cold");
                None
            }
        };
        let payload = match held.as_deref().map(|bytes| decode(&key, bytes)) {
            Some(Ok(payload)) => payload,
            Some(Err(refusal)) => {
                tracing::info!(
                    ?refusal,
                    "the stored pipeline cache is refused; starting cold"
                );
                empty(&*storage);
                None
            }
            None => None,
        };
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let (cache, seeded) = create_cache(device, payload);
        let (cache, seeded) = match scope.pop().await {
            None => (cache, seeded),
            // wgpu refused data the driver did not make for this device
            // (`DeviceMismatch`); the cache it answered with is an error
            // value, so a cold one replaces it.
            Some(error) => {
                tracing::info!(%error, "wgpu refused the stored pipeline cache; starting cold");
                create_cache(device, None)
            }
        };
        tracing::debug!(seeded, "pipeline cache created");
        Some(Self {
            cache,
            key: key.into(),
            storage,
            #[cfg(test)]
            seeded,
        })
    }

    /// A cold cache on `device`, a rebuilt device on `adapter`, saved to the
    /// same storage; `None` where that device has no pipeline cache.
    pub(crate) fn restart(
        self,
        device: &wgpu::Device,
        adapter: &wgpu::AdapterInfo,
    ) -> Option<Self> {
        let key = cache_key(device, adapter)?;
        Some(Self {
            cache: create_cache(device, None).0,
            key: key.into(),
            storage: self.storage,
            #[cfg(test)]
            seeded: false,
        })
    }

    /// The cache every pipeline of the context is created through.
    pub(crate) fn cache(&self) -> &wgpu::PipelineCache {
        &self.cache
    }

    /// Whether wgpu took the stored data the cache was created from.
    #[cfg(test)]
    pub(crate) fn seeded(&self) -> bool {
        self.seeded
    }

    /// Hand the cache's data to the storage. The storage accepts it before
    /// this returns and writes it on its own threads; how the write ends is
    /// not awaited, since a lost write costs the next launch only the
    /// compilation it would have saved.
    ///
    /// Runs the storage's code, which may panic.
    pub(crate) fn save(&self) {
        let Some(payload) = self.cache.get_data() else {
            return;
        };
        let Some(envelope) = encode(&self.key, &payload) else {
            tracing::warn!(
                len = payload.len(),
                "the pipeline cache is too large to save"
            );
            return;
        };
        drop(payload);
        drop(
            self.storage
                .publish(&PIPELINE_CACHE, envelope, WriteMode::Replace),
        );
    }
}

/// The key `adapter`'s cache data is stored under, when `device` has a
/// pipeline cache.
fn cache_key(device: &wgpu::Device, adapter: &wgpu::AdapterInfo) -> Option<String> {
    if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        return None;
    }
    wgpu::util::pipeline_cache_key(adapter)
}

/// Replace the stored value with an empty one, which [`decode`] reads as
/// absent.
fn empty(storage: &dyn Storage) {
    drop(storage.publish(&PIPELINE_CACHE, Vec::new(), WriteMode::Replace));
}

/// A pipeline cache on `device`, from `data` when given, and whether it was.
#[expect(
    unsafe_code,
    reason = "wgpu creates a pipeline cache only through an unsafe call; its bytes come from a checked envelope"
)]
fn create_cache(device: &wgpu::Device, data: Option<&[u8]>) -> (wgpu::PipelineCache, bool) {
    // SAFETY: `create_pipeline_cache` requires `data` to be bytes that
    // `PipelineCache::get_data` returned on an adapter with the same
    // `pipeline_cache_key`. `data` is `None`, or the payload `decode`
    // returned: `save` wrapped exactly what `get_data` returned with the key
    // of the device's adapter, and `decode` returns it only when the stored
    // key equals this adapter's key (`load` derives both with `cache_key`) and
    // the SHA-256 digest over key and payload matches, so the bytes are the
    // ones `get_data` returned for this adapter key, unaltered. The digest
    // proves integrity, not origin: the value lives in the application's own
    // machine-local data directory, which the platform store creates for its
    // owner alone, so the only writer of an envelope is `save`.
    let cache = unsafe {
        device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
            label: Some("FLUI Pipeline Cache"),
            data,
            fallback: true,
        })
    };
    (cache, data.is_some())
}

#[cfg(test)]
mod tests {
    use super::{MAX_ENVELOPE_BYTES, Refusal, decode, encode};

    const KEY: &str = "wgpu_pipeline_cache_vulkan_4318_7938";
    const PAYLOAD: &[u8] = b"compiled pipelines";

    fn envelope() -> Vec<u8> {
        encode(KEY, PAYLOAD).expect("a small payload fits an envelope")
    }

    /// An envelope decodes to the payload it was made from, for its key; an
    /// empty value is absent.
    #[test]
    fn an_envelope_returns_its_payload_for_its_key() {
        assert_eq!(decode(KEY, &envelope()), Ok(Some(PAYLOAD)));
        assert_eq!(decode(KEY, &[]), Ok(None));
        let empty_payload = encode(KEY, &[]).expect("an empty payload fits");
        assert_eq!(decode(KEY, &empty_payload), Ok(Some(&[][..])));
    }

    /// Each way stored bytes can fail to be this adapter's cache is refused
    /// with its own reason, and no payload is returned.
    #[test]
    fn bytes_the_envelope_does_not_prove_are_refused_with_their_reason() {
        let whole = envelope();
        let mut flipped = whole.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 0x01;
        let mut other_tag = whole.clone();
        other_tag[7] = b'2';
        let mut extended = whole.clone();
        extended.push(0);
        let cases: [(&str, &[u8], &str, Refusal); 7] = [
            (
                "payload cut short",
                &whole[..whole.len() - 1],
                KEY,
                Refusal::Truncated,
            ),
            ("header cut short", &whole[..10], KEY, Refusal::Truncated),
            ("a byte appended", &extended, KEY, Refusal::Extended),
            ("a payload byte flipped", &flipped, KEY, Refusal::Corrupt),
            ("another format version", &other_tag, KEY, Refusal::Tag),
            (
                "another adapter's key",
                &whole,
                "wgpu_pipeline_cache_vulkan_4318_7939",
                Refusal::OtherAdapter,
            ),
            ("not an envelope", b"garbage", KEY, Refusal::Truncated),
        ];
        for (case, bytes, key, refusal) in cases {
            assert_eq!(decode(key, bytes), Err(refusal), "{case}");
        }
        assert_eq!(decode(KEY, b"not a pipeline cache"), Err(Refusal::Tag));
    }

    /// Nothing over the limit is decoded or encoded, so a run never writes
    /// what the next one refuses to read.
    #[test]
    fn an_envelope_over_the_limit_is_neither_read_nor_written() {
        let limit = usize::try_from(MAX_ENVELOPE_BYTES).expect("64 MiB fits usize");
        let over = vec![0_u8; limit + 1];
        assert_eq!(decode(KEY, &over), Err(Refusal::TooLarge));
        assert_eq!(encode(KEY, &over[..limit]), None);
        let fits = limit - (8 + 4 + KEY.len() + 8 + 32);
        let largest = encode(KEY, &over[..fits]).expect("the largest payload that fits");
        assert_eq!(largest.len(), limit);
        assert_eq!(decode(KEY, &largest), Ok(Some(&over[..fits])));
    }
}
