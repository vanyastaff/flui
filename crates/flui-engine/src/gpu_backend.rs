//! Which GPU backend the renderer draws with, and the order it tries them in
//! (ADR-0244 §1).
//!
//! The GPU API is a probe, not a floor: whether a device has a working Vulkan
//! driver or a browser a WebGPU adapter depends on the device, so the renderer
//! asks for the preferred backend and falls back to GLES 3.0 (WebGL2 on the
//! web) when no adapter answers. Each attempt has an instance of its own for
//! that one backend, because a combined backend mask does not retry: on the
//! web, wgpu commits to its WebGPU context as soon as `navigator.gpu` exists,
//! so a browser that exposes it but returns no adapter reaches WebGL2 only
//! through a second, GL-only instance.

use std::fmt;

/// A GPU backend the renderer can draw with.
///
/// FLUI's own name for it, so the renderer's public surface does not carry
/// wgpu's (ADR-0089). [`GpuCapabilities::backend`](crate::GpuCapabilities::backend)
/// says which one a renderer chose; [`EngineError::NoSupportedGpu`](crate::EngineError::NoSupportedGpu)
/// says why each one it tried failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GpuBackend {
    /// Vulkan: Linux and Android's preferred backend.
    Vulkan,
    /// Metal: macOS and iOS.
    Metal,
    /// Direct3D 12 on a hardware adapter: Windows' preferred backend.
    Dx12,
    /// Direct3D 12 on WARP, Windows' software rasterizer, for a machine
    /// without a WDDM 2 driver.
    Dx12Warp,
    /// OpenGL ES 3.0: the fallback on Linux and Android.
    Gl,
    /// WebGPU: the web's preferred backend.
    WebGpu,
    /// WebGL2: the fallback on the web.
    WebGl2,
}

impl GpuBackend {
    /// The backend an adapter wgpu returned runs on.
    pub(crate) fn of_adapter(info: &wgpu::AdapterInfo) -> Self {
        match info.backend {
            wgpu::Backend::Vulkan => Self::Vulkan,
            wgpu::Backend::Metal => Self::Metal,
            wgpu::Backend::Dx12 if info.device_type == wgpu::DeviceType::Cpu => Self::Dx12Warp,
            wgpu::Backend::Dx12 => Self::Dx12,
            wgpu::Backend::Gl if cfg!(target_arch = "wasm32") => Self::WebGl2,
            // No adapter has the no-op backend: it is never enabled.
            wgpu::Backend::Gl | wgpu::Backend::Noop => Self::Gl,
            wgpu::Backend::BrowserWebGpu => Self::WebGpu,
        }
    }
}

impl fmt::Display for GpuBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Vulkan => "Vulkan",
            Self::Metal => "Metal",
            Self::Dx12 => "Direct3D 12",
            Self::Dx12Warp => "Direct3D 12 (WARP)",
            Self::Gl => "OpenGL ES",
            Self::WebGpu => "WebGPU",
            Self::WebGl2 => "WebGL2",
        })
    }
}

/// Why one backend the renderer tried gave it no device.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BackendFailure {
    /// The backend tried.
    pub backend: GpuBackend,
    /// What failed, in wgpu's or the driver's words: no adapter, a surface it
    /// could not make, a device it refused.
    pub reason: String,
}

impl fmt::Display for BackendFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.backend, self.reason)
    }
}

/// One backend the renderer tries: an instance over `backends` alone, asking
/// for the software adapter when `software` is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Attempt {
    pub(crate) backend: GpuBackend,
    pub(crate) backends: wgpu::Backends,
    pub(crate) software: bool,
}

/// The targets that share an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    /// macOS and iOS.
    Apple,
    /// Windows.
    Windows,
    /// Linux and Android, and a target FLUI names no backend for.
    Vulkan,
    /// The web.
    Web,
}

impl Family {
    /// The family of the target this is built for.
    pub(crate) const CURRENT: Self = if cfg!(any(target_os = "macos", target_os = "ios")) {
        Self::Apple
    } else if cfg!(target_os = "windows") {
        Self::Windows
    } else if cfg!(target_arch = "wasm32") {
        Self::Web
    } else {
        Self::Vulkan
    };

    /// The backends this family tries, preferred first.
    pub(crate) const fn order(self) -> &'static [Attempt] {
        match self {
            Self::Apple => &[Attempt {
                backend: GpuBackend::Metal,
                backends: wgpu::Backends::METAL,
                software: false,
            }],
            Self::Windows => &[
                Attempt {
                    backend: GpuBackend::Dx12,
                    backends: wgpu::Backends::DX12,
                    software: false,
                },
                Attempt {
                    backend: GpuBackend::Dx12Warp,
                    backends: wgpu::Backends::DX12,
                    software: true,
                },
            ],
            Self::Vulkan => &[
                Attempt {
                    backend: GpuBackend::Vulkan,
                    backends: wgpu::Backends::VULKAN,
                    software: false,
                },
                Attempt {
                    backend: GpuBackend::Gl,
                    backends: wgpu::Backends::GL,
                    software: false,
                },
            ],
            Self::Web => &[
                Attempt {
                    backend: GpuBackend::WebGpu,
                    backends: wgpu::Backends::BROWSER_WEBGPU,
                    software: false,
                },
                Attempt {
                    backend: GpuBackend::WebGl2,
                    backends: wgpu::Backends::GL,
                    software: false,
                },
            ],
        }
    }
}

/// The first of `attempts` that `try_one` gets a device from, with the
/// backend it ran on, or why each failed: `try_one` answers an attempt with
/// its device, or with what went wrong in words.
pub(crate) async fn first_answering<T, F, Fut>(
    attempts: &[Attempt],
    mut try_one: F,
) -> Result<(GpuBackend, T), Vec<BackendFailure>>
where
    F: FnMut(Attempt) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let mut failures = Vec::with_capacity(attempts.len());
    for attempt in attempts {
        match try_one(*attempt).await {
            Ok(device) => return Ok((attempt.backend, device)),
            Err(reason) => {
                tracing::warn!(backend = %attempt.backend, %reason, "GPU backend unavailable");
                failures.push(BackendFailure {
                    backend: attempt.backend,
                    reason,
                });
            }
        }
    }
    Err(failures)
}

/// The attempts of `order` that `forced` allows: all of them when nothing is
/// forced, otherwise those whose backend `forced` names. `forced` is
/// wgpu's own `WGPU_BACKEND` (`wgpu::Backends::from_env`), which is how a
/// test runs the same suite on the fallback.
pub(crate) fn attempts(order: &[Attempt], forced: Option<wgpu::Backends>) -> Vec<Attempt> {
    order
        .iter()
        .copied()
        .filter(|attempt| forced.is_none_or(|forced| forced.intersects(attempt.backends)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Attempt, BackendFailure, Family, GpuBackend, attempts, first_answering};

    /// Answers the attempts in `answering` with a device and fails the rest,
    /// recording the order it was asked in.
    fn run(
        family: Family,
        forced: Option<wgpu::Backends>,
        answering: &[GpuBackend],
    ) -> (Vec<GpuBackend>, Result<GpuBackend, Vec<BackendFailure>>) {
        let mut asked = Vec::new();
        let result = pollster::block_on(first_answering(
            &attempts(family.order(), forced),
            |attempt: Attempt| {
                asked.push(attempt.backend);
                let answers = answering.contains(&attempt.backend);
                async move {
                    if answers {
                        Ok(())
                    } else {
                        Err("no adapter".to_owned())
                    }
                }
            },
        ))
        .map(|(backend, ())| backend);
        (asked, result)
    }

    /// One row: a family, the backend `WGPU_BACKEND` forces, the backends
    /// that answer, then the order they must be asked in and the one chosen
    /// (`None` when none answers).
    struct Case {
        family: Family,
        forced: Option<wgpu::Backends>,
        answering: &'static [GpuBackend],
        asked: &'static [GpuBackend],
        chosen: Option<GpuBackend>,
    }

    /// The preferred backend first, then GL in an instance of its own (WebGL2
    /// on the web), WARP after hardware Direct3D 12; a forced backend keeps
    /// only its attempts; when none answers, every attempt is named with its
    /// reason.
    #[test]
    fn gpu_backend_fallback_order() {
        use GpuBackend::{Dx12, Dx12Warp, Gl, Metal, Vulkan, WebGl2, WebGpu};

        let case = |family, forced, answering, asked, chosen| Case {
            family,
            forced,
            answering,
            asked,
            chosen,
        };
        let cases = [
            case(Family::Vulkan, None, &[Vulkan, Gl], &[Vulkan], Some(Vulkan)),
            case(Family::Vulkan, None, &[Gl], &[Vulkan, Gl], Some(Gl)),
            case(
                Family::Vulkan,
                Some(wgpu::Backends::GL),
                &[Vulkan, Gl],
                &[Gl],
                Some(Gl),
            ),
            case(Family::Vulkan, None, &[], &[Vulkan, Gl], None),
            case(
                Family::Web,
                None,
                &[WebGl2],
                &[WebGpu, WebGl2],
                Some(WebGl2),
            ),
            case(
                Family::Windows,
                None,
                &[Dx12Warp],
                &[Dx12, Dx12Warp],
                Some(Dx12Warp),
            ),
            case(Family::Apple, None, &[Metal], &[Metal], Some(Metal)),
            case(Family::Apple, None, &[], &[Metal], None),
        ];
        let mut failures = Vec::new();
        for (index, case) in cases.iter().enumerate() {
            let (asked, result) = run(case.family, case.forced, case.answering);
            if asked != case.asked {
                failures.push(format!("case {index}: asked {asked:?}"));
            }
            match (result, case.chosen) {
                (Ok(chosen), Some(expected)) if chosen == expected => {}
                (Err(tried), None)
                    if tried
                        .iter()
                        .map(|failure| failure.backend)
                        .collect::<Vec<_>>()
                        == case.asked
                        && tried.iter().all(|failure| failure.reason == "no adapter") => {}
                (result, _) => failures.push(format!("case {index}: {result:?}")),
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
