//! The GPU execution provider for each platform, behind one interface for `siglip`:
//! DirectML on Windows, WebGPU (Metal) on macOS, none elsewhere (the tests on Linux use the CPU).
//!
//! - `NAME`: the provider's name, as reported in the output's `backend=` field, or None.
//! - `LIBRARY`: ONNX Runtime's library file name, loaded from the exe's folder.
//! - `preload(dir)`: loads what ONNX Runtime needs before it is loaded itself.
//! - `configure(builder, model)`: adds the provider to a session for `model`.
//! - `device_name()`: what the provider runs on, for the report.

pub use imp::{configure, device_name, preload, LIBRARY, NAME};

#[cfg(any(windows, target_os = "macos"))]
fn ort_error(e: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("ONNX Runtime: {e}")
}

#[cfg(windows)]
mod imp {
    //! DirectML: any DirectX 12 GPU. With more than one (a desktop CPU's integrated graphics
    //! and a graphics card, say), the one Windows ranks as high-performance.
    use std::path::Path;

    use anyhow::{anyhow, Result};
    use ort::ep;
    use ort::session::builder::SessionBuilder;

    use super::ort_error;

    pub const NAME: Option<&str> = Some("DirectML");
    pub const LIBRARY: &str = "onnxruntime.dll";

    /// Loads our DirectML.dll before ONNX Runtime asks for it, so an older copy in System32
    /// isn't used.
    pub fn preload(dir: &Path) -> Result<()> {
        let directml = dir.join("DirectML.dll");
        if directml.is_file() {
            ort::util::preload_dylib(&directml).map_err(|e| anyhow!("cannot load {}: {e}", directml.display()))?;
        }
        Ok(())
    }

    pub fn configure(builder: SessionBuilder, _model: &Path) -> Result<SessionBuilder> {
        // DirectML doesn't support memory patterns or parallel execution.
        builder
            .with_memory_pattern(false)
            .map_err(ort_error)?
            .with_parallel_execution(false)
            .map_err(ort_error)?
            .with_execution_providers([ep::DirectML::default()
                .with_performance_preference(ep::directml::PerformancePreference::HighPerformance)
                .build()
                .error_on_failure()])
            .map_err(ort_error)
    }

    /// The name of the GPU Windows ranks first for high performance: the one DirectML picks
    /// with `PerformancePreference::HighPerformance` (it sorts adapters the same way).
    pub fn device_name() -> Option<String> {
        use windows::Win32::Graphics::Dxgi::{
            CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory6, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
        };
        // SAFETY: plain DXGI calls; the COM wrappers release the objects when dropped.
        let desc = unsafe {
            let factory: IDXGIFactory6 = CreateDXGIFactory1().ok()?;
            let adapter: IDXGIAdapter1 =
                factory.EnumAdapterByGpuPreference(0, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE).ok()?;
            adapter.GetDesc1().ok()?
        };
        let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..len]).trim().to_string();
        (!name.is_empty()).then_some(name)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    //! WebGPU, which ONNX Runtime runs on Metal: the Apple Silicon GPU. CoreML was tried
    //! first and isn't used: its NeuralNetwork format takes 14 of the vision model's 829 nodes
    //! (no faster than the CPU), and its MLProgram format can't compile the model's Conv.
    //! WebGPU is about twice as fast as the CPU here and needs no compile step.
    use std::ffi::CString;
    use std::path::Path;

    use anyhow::Result;
    use ort::ep;
    use ort::session::builder::SessionBuilder;

    use super::ort_error;

    pub const NAME: Option<&str> = Some("WebGPU");
    pub const LIBRARY: &str = "libonnxruntime.dylib";

    pub fn preload(_dir: &Path) -> Result<()> {
        Ok(())
    }

    pub fn configure(builder: SessionBuilder, _model: &Path) -> Result<SessionBuilder> {
        builder.with_execution_providers([ep::WebGPU::default().build().error_on_failure()]).map_err(ort_error)
    }

    /// The chip, e.g. "Apple M3 Pro".
    pub fn device_name() -> Option<String> {
        let key = CString::new("machdep.cpu.brand_string").ok()?;
        let mut buf = [0u8; 128];
        let mut len = buf.len();
        // SAFETY: sysctlbyname writes at most `len` bytes into `buf` and updates `len`.
        let status =
            unsafe { libc::sysctlbyname(key.as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) };
        if status != 0 {
            return None;
        }
        let name = String::from_utf8_lossy(&buf[..len]).trim_end_matches('\0').trim().to_string();
        (!name.is_empty()).then_some(name)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    //! No GPU provider: the CPU is used.
    use std::path::Path;

    use anyhow::{bail, Result};
    use ort::session::builder::SessionBuilder;

    pub const NAME: Option<&str> = None;
    pub const LIBRARY: &str = "libonnxruntime.so";

    pub fn preload(_dir: &Path) -> Result<()> {
        Ok(())
    }

    pub fn configure(_builder: SessionBuilder, _model: &Path) -> Result<SessionBuilder> {
        bail!("no GPU provider on this platform")
    }

    pub fn device_name() -> Option<String> {
        None
    }
}
