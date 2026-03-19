use std::sync::Arc;

/// GPU-accelerated rendering surface backed by wgpu.
///
/// Manages the wgpu Device, Queue, and Surface for a single window.
/// Provides clear-to-color drawing and resize handling without panicking
/// on recoverable surface errors (Lost, Outdated).
pub struct GlassRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    consecutive_surface_failures: u32,
}

impl GlassRenderer {
    /// Initialize the wgpu instance, adapter, device, queue, and surface for the given window.
    ///
    /// Returns an error with a user-friendly message if GPU initialization fails (e.g. no
    /// compatible adapter, missing driver, headless VM). On Windows this auto-selects the
    /// DX12 backend. The selected backend is logged via `tracing::info!`.
    pub async fn try_new(
        window: Arc<winit::window::Window>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Prefer DX12 on Windows (faster init than Vulkan), Metal on macOS, Vulkan on Linux.
        // On Windows, also allow Vulkan as a fallback for older GPUs without DX12 support.
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            #[cfg(target_os = "windows")]
            backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
            #[cfg(not(target_os = "windows"))]
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).map_err(|e| {
            format!(
                "Failed to create GPU surface: {e}. \
                 Your GPU driver may not support the required graphics API. \
                 Run `glass check` for diagnosis."
            )
        })?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| {
                format!(
                    "No compatible GPU adapter found: {e}. \
                 Ensure your system has a GPU with DX12 (Windows), Metal (macOS), \
                 or Vulkan (Linux) support. Run `glass check` for diagnosis."
                )
            })?;

        // Log which backend was selected — on Windows 11 this should be Dx12
        tracing::info!("GPU backend: {:?}", adapter.get_info().backend);

        let (device, queue): (wgpu::Device, wgpu::Queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| {
                format!(
                    "Failed to create GPU device: {e}. \
                 Your GPU driver may be outdated. Run `glass check` for diagnosis."
                )
            })?;

        let adapter_info = adapter.get_info();
        tracing::info!(
            "GPU adapter: {} ({:?})",
            adapter_info.name,
            adapter_info.backend
        );

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);

        tracing::info!("Available surface formats: {:?}", caps.formats);

        // Prefer sRGB format for consistent color rendering across platforms
        let format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);

        tracing::info!("Selected surface format: {:?}", format);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 1,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        Ok(Self {
            device,
            queue,
            surface,
            surface_config,
            consecutive_surface_failures: 0,
        })
    }

    /// Convenience wrapper around `try_new` that shows a fatal error and exits on failure.
    ///
    /// Kept for backward compatibility with call sites that do not handle errors.
    pub async fn new(window: Arc<winit::window::Window>) -> Self {
        Self::try_new(window).await.unwrap_or_else(|e| {
            eprintln!("Glass fatal GPU error: {e}");
            #[cfg(target_os = "windows")]
            {
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    MessageBoxW, MB_ICONERROR, MB_OK,
                };
                let msg: Vec<u16> = format!("{e}")
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                let title: Vec<u16> = "Glass - GPU Error"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                unsafe {
                    MessageBoxW(
                        std::ptr::null_mut(),
                        msg.as_ptr(),
                        title.as_ptr(),
                        MB_ICONERROR | MB_OK,
                    );
                }
            }
            std::process::exit(1);
        })
    }

    /// Render a single frame clearing the surface to dark gray (0.1, 0.1, 0.1).
    ///
    /// Handles `SurfaceError::Lost` and `SurfaceError::Outdated` by reconfiguring
    /// the surface and skipping the frame — does NOT panic on these recoverable errors.
    pub fn draw(&mut self) {
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => {
                self.consecutive_surface_failures = 0;
                frame
            }
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                // Recoverable — reconfigure and skip this frame (Pitfall 3)
                self.surface.configure(&self.device, &self.surface_config);
                self.consecutive_surface_failures += 1;
                if self.consecutive_surface_failures >= 3 {
                    tracing::warn!("3 consecutive GPU surface failures — device may be lost");
                    self.consecutive_surface_failures = 0;
                }
                return;
            }
            Err(e) => {
                tracing::error!("Surface error: {e}");
                return;
            }
        };

        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());

        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        self.queue.submit([encoder.finish()]);
        frame.present();
    }

    /// Get a reference to the wgpu device.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Get a reference to the wgpu queue.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Get the surface texture format.
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_config.format
    }

    /// Get a reference to the surface configuration.
    pub fn surface_config(&self) -> &wgpu::SurfaceConfiguration {
        &self.surface_config
    }

    /// Get the current surface texture for rendering.
    ///
    /// Returns None if the surface texture could not be acquired (recoverable error).
    /// On Lost/Outdated, reconfigures the surface automatically.
    pub fn get_current_texture(&mut self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            Ok(frame) => Some(frame),
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.surface_config);
                None
            }
            Err(e) => {
                tracing::error!("Surface error: {e}");
                None
            }
        }
    }

    /// Update the surface configuration to match the new window size.
    ///
    /// Returns early (no-op) if either dimension is zero, which can occur
    /// during minimization on some platforms.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }
}
