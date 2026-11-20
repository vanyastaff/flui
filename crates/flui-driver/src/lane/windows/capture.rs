//! The OS's own capture of an app's window, through Windows Graphics Capture: what the compositor
//! shows of that one window, cut to its client area, never the screen around it.
//!
//! A capture item is made for the window, a free-threaded frame pool of one buffer receives what
//! the compositor draws of it, and the first frame within [`FRAME_WITHIN`] is copied to a staging
//! texture and read back. The device, the pool and the session live for one capture, on the
//! lane's thread.
#![expect(
    unsafe_code,
    reason = "Direct3D 11 and Windows Graphics Capture interop; each call takes a live interface \
              or a local"
)]

use std::time::{Duration, Instant};

use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::core::Interface;

use crate::error::Result;
use crate::lane::windows::win32::{Apartment, Hwnd, client_area, lane_error};
use crate::pixels::Image;

/// How long a capture waits for the compositor's first frame of the window.
pub const FRAME_WITHIN: Duration = Duration::from_secs(3);

/// How often the pool is asked for a frame while waiting.
const POLL: Duration = Duration::from_millis(10);

fn failed(what: &str) -> impl FnOnce(windows::core::Error) -> crate::error::Error + '_ {
    move |error| lane_error(format!("capturing the app's window: {what}: {error}"))
}

/// The client area of `window` as the compositor shows it: see the [module docs](self).
///
/// # Errors
///
/// When Direct3D or Windows Graphics Capture is unavailable, the compositor sends no frame in
/// time, or the window's client area is not inside the frame.
pub(super) fn capture(window: Hwnd) -> Result<Image> {
    let _apartment = Apartment::enter()?;
    let (device, context) = device()?;
    let dxgi: IDXGIDevice = device.cast().map_err(failed("the DXGI device"))?;
    // SAFETY: a live DXGI device of this thread.
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
        .map_err(failed("the WinRT device"))?;
    let winrt: IDirect3DDevice = inspectable.cast().map_err(failed("the WinRT device"))?;
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(failed("the capture factory"))?;
    // SAFETY: a window handle the lane found, and the item type the interop makes.
    let item: GraphicsCaptureItem =
        unsafe { interop.CreateForWindow(window.raw()) }.map_err(failed("the capture item"))?;
    let size = item.Size().map_err(failed("the item's size"))?;
    if size.Width <= 0 || size.Height <= 0 {
        // A host whose compositor does not show the window cannot make the capture at all.
        return Err(crate::error::Error::Lane {
            lane: crate::lane::windows::safety::LANE.to_owned(),
            unavailable: true,
            message: format!(
                "capturing the app's window: the compositor gives it no area ({} x {}), as on a \
                 desktop it does not show",
                size.Width, size.Height
            ),
        });
    }
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &winrt,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        size,
    )
    .map_err(failed("the frame pool"))?;
    let session = pool
        .CreateCaptureSession(&item)
        .map_err(failed("the capture session"))?;
    // Best effort: the yellow border and the cursor are not the app's.
    let _ = session.SetIsBorderRequired(false);
    let _ = session.SetIsCursorCaptureEnabled(false);
    session
        .StartCapture()
        .map_err(failed("starting the session"))?;
    let deadline = Instant::now() + FRAME_WITHIN;
    let frame = loop {
        if let Ok(frame) = pool.TryGetNextFrame() {
            break frame;
        }
        if Instant::now() >= deadline {
            let _ = session.Close();
            let _ = pool.Close();
            return Err(lane_error(format!(
                "capturing the app's window: the compositor sent no frame within {} ms",
                FRAME_WITHIN.as_millis()
            )));
        }
        std::thread::sleep(POLL);
    };
    let read = (|| {
        let access: IDirect3DDxgiInterfaceAccess = frame
            .Surface()
            .and_then(|surface| surface.cast())
            .map_err(failed("the frame's surface"))?;
        // SAFETY: the surface of a frame this pool returned, alive while `frame` is.
        let texture: ID3D11Texture2D =
            unsafe { access.GetInterface() }.map_err(failed("the frame's texture"))?;
        read_client(window, &device, &context, &texture)
    })();
    let _ = frame.Close();
    let _ = session.Close();
    let _ = pool.Close();
    read
}

fn device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    // SAFETY: the default adapter, out-parameters that are locals.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&raw mut device),
            None,
            Some(&raw mut context),
        )
    }
    .map_err(failed("the Direct3D device"))?;
    match (device, context) {
        (Some(device), Some(context)) => Ok((device, context)),
        _ => Err(lane_error(
            "capturing the app's window: Direct3D made no device",
        )),
    }
}

/// The window's client area out of `texture`, which holds the window's frame bounds.
fn read_client(
    window: Hwnd,
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    texture: &ID3D11Texture2D,
) -> Result<Image> {
    let (client, width, height, _) = client_area(window)?;
    let mut frame = RECT::default();
    // SAFETY: a window handle and a local `RECT` with its size.
    unsafe {
        DwmGetWindowAttribute(
            window.raw(),
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut frame).cast(),
            u32::try_from(size_of::<RECT>()).expect("BUG: a RECT is sixteen bytes"),
        )
    }
    .map_err(failed("the window's frame bounds"))?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: a live texture; `desc` is a local the call writes.
    unsafe { texture.GetDesc(&raw mut desc) };
    let left = u32::try_from(client.x - frame.left).ok();
    let top = u32::try_from(client.y - frame.top).ok();
    let (width, height) = (u32::try_from(width).ok(), u32::try_from(height).ok());
    let (Some(left), Some(top), Some(width), Some(height)) = (left, top, width, height) else {
        return Err(lane_error(
            "capturing the app's window: its client area lies outside its frame",
        ));
    };
    if width == 0 || height == 0 || left + width > desc.Width || top + height > desc.Height {
        return Err(lane_error(format!(
            "capturing the app's window: its client area ({left}, {top}, {width} x {height}) is \
             not inside the {} x {} frame",
            desc.Width, desc.Height
        )));
    }
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        BindFlags: 0,
        MiscFlags: 0,
        Usage: D3D11_USAGE_STAGING,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0.cast_unsigned(),
        ..desc
    };
    let mut staging = None;
    // SAFETY: a live device; a full description and a local out-parameter.
    unsafe { device.CreateTexture2D(&raw const staging_desc, None, Some(&raw mut staging)) }
        .map_err(failed("the staging texture"))?;
    let staging: ID3D11Texture2D =
        staging.ok_or_else(|| lane_error("capturing the app's window: no staging texture"))?;
    let region = D3D11_BOX {
        left,
        top,
        front: 0,
        right: left + width,
        bottom: top + height,
        back: 1,
    };
    let destination: ID3D11Resource = staging.cast().map_err(failed("the staging texture"))?;
    let source: ID3D11Resource = texture.cast().map_err(failed("the frame's texture"))?;
    // SAFETY: two live textures of one device; the region lies inside the source (checked
    // above) and fits the destination, which is its size.
    unsafe {
        context.CopySubresourceRegion(
            &destination,
            0,
            0,
            0,
            0,
            &source,
            0,
            Some(&raw const region),
        );
    }
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: a staging texture with CPU read access, mapped once and unmapped below.
    unsafe { context.Map(&destination, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped)) }
        .map_err(failed("reading the staging texture"))?;
    let row = usize::try_from(width).expect("BUG: a width fits usize") * 4;
    let pitch = usize::try_from(mapped.RowPitch).expect("BUG: a row pitch fits usize");
    let rows = usize::try_from(height).expect("BUG: a height fits usize");
    let mut rgba = Vec::with_capacity(row * rows);
    for y in 0..rows {
        // SAFETY: the mapping holds `height` rows of `RowPitch` bytes, each at least `row` bytes
        // of the B8G8R8A8 texel data, alive until `Unmap`.
        let bgra =
            unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>().add(y * pitch), row) };
        for [blue, green, red, alpha] in bgra.as_chunks::<4>().0 {
            rgba.extend_from_slice(&[*red, *green, *blue, *alpha]);
        }
    }
    // SAFETY: the subresource mapped above.
    unsafe { context.Unmap(&destination, 0) };
    Image::try_new(width, height, rgba)
}
