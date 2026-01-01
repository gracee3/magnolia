#[cfg(feature = "tile-rendering")]
use nannou::prelude::*;
pub use magnolia_core::KameaGrid;
#[cfg(feature = "tile-rendering")]
use magnolia_plugin_helper::SignalValue;
use magnolia_plugin_helper::{export_plugin, SignalBuffer, SignalType, MagnoliaPlugin};

#[cfg(feature = "tile-rendering")]
use nannou::wgpu; // Access nannou's re-exported wgpu

mod generator;
mod tile;
use tile::KameaTile;

#[cfg(feature = "tile-rendering")]
struct GpuState {
    device: *const wgpu::Device,
    queue: *const wgpu::Queue,
    renderer: nannou::draw::Renderer,
    nannou_texture: wgpu::Texture,
    view: wgpu::TextureView, // Store the view to keep it alive
    width: u32,
    height: u32,
}

#[cfg(feature = "tile-rendering")]
unsafe impl Send for GpuState {}
#[cfg(feature = "tile-rendering")]
unsafe impl Sync for GpuState {}

struct KameaPlugin {
    tile: Option<KameaTile>,
    #[cfg(feature = "tile-rendering")]
    gpu_state: Option<GpuState>,
    enabled: bool,
    #[cfg(feature = "tile-rendering")]
    sent_texture: bool,
}

impl Default for KameaPlugin {
    fn default() -> Self {
        Self {
            tile: None,
            #[cfg(feature = "tile-rendering")]
            gpu_state: None,
            enabled: true,
            #[cfg(feature = "tile-rendering")]
            sent_texture: false,
        }
    }
}

impl MagnoliaPlugin for KameaPlugin {
    fn name() -> &'static str {
        "kamea"
    }
    fn version() -> &'static str {
        "0.1.0"
    }
    fn description() -> &'static str {
        "Generative Sigil Visualizer"
    }
    fn author() -> &'static str {
        "Magnolia"
    }

    fn c_id(&self) -> *const std::os::raw::c_char {
        b"kamea\0".as_ptr() as *const _
    }
    fn c_name(&self) -> *const std::os::raw::c_char {
        b"Kamea Sigil\0".as_ptr() as *const _
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn poll_signal(
        &mut self,
        #[cfg_attr(not(feature = "tile-rendering"), allow(unused_variables))]
        buffer: &mut SignalBuffer,
    ) -> bool {
        if !self.enabled {
            return false;
        }

        if self.tile.is_none() {
            self.tile = Some(KameaTile::new());
        }

        #[cfg(feature = "tile-rendering")]
        {
            if let Some(gpu) = &mut self.gpu_state {
                let tile = self.tile.as_mut().unwrap();
                use magnolia_core::TileRenderer;
                tile.update();

                let draw = Draw::new();
                let rect = Rect::from_w_h(gpu.width as f32, gpu.height as f32);

                let ctx = magnolia_core::RenderContext {
                    time: std::time::Instant::now(),
                    frame_count: 0,
                    is_selected: false,
                    is_maximized: false,
                    tile_settings: None,
                };

                tile.render_monitor(&draw, rect, &ctx);

                unsafe {
                    let device = &*gpu.device;
                    let queue = &*gpu.queue;

                    let mut encoder =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("kamea_encoder"),
                        });

                    gpu.renderer.render_to_texture(
                        device,
                        &mut encoder,
                        &draw,
                        &gpu.nannou_texture,
                    );

                    queue.submit(Some(encoder.finish()));
                }

                if !self.sent_texture {
                    let id = 0xCAFE_BABE;

                    buffer.signal_type = SignalType::Texture as u32;
                    let raw_view = gpu.view.inner();

                    buffer.value = SignalValue {
                        ptr: raw_view as *const _ as *mut _,
                    };
                    buffer.size = id;
                    buffer.param = ((gpu.width as u64) << 32) | (gpu.height as u64);

                    self.sent_texture = true;
                    return true;
                }
            }
        }

        false
    }

    fn consume_signal(&mut self, input: &SignalBuffer) -> Option<SignalBuffer> {
        if input.signal_type == SignalType::GpuContext as u32 {
            #[cfg(feature = "tile-rendering")]
            unsafe {
                let device_ptr = input.value.ptr as *const wgpu::Device;
                let queue_ptr = input.param as *const wgpu::Queue;

                if !device_ptr.is_null() && !queue_ptr.is_null() {
                    self.init_gpu(device_ptr, queue_ptr);
                }
            }
        } else if input.signal_type == SignalType::Text as u32 {
            unsafe {
                if !input.value.ptr.is_null() {
                    use std::ffi::CStr;
                    // Cast void* to char*
                    let ptr = input.value.ptr as *const std::os::raw::c_char;
                    if let Ok(c_str) = CStr::from_ptr(ptr).to_str() {
                        if self.tile.is_none() {
                            self.tile = Some(KameaTile::new());
                        }
                        if let Some(tile) = &self.tile {
                            tile.set_text(c_str);
                        }
                    }
                }
            }
        }
        None
    }
}

#[cfg(feature = "tile-rendering")]
impl KameaPlugin {
    unsafe fn init_gpu(&mut self, device_ptr: *const wgpu::Device, queue_ptr: *const wgpu::Queue) {
        let device = &*device_ptr;

        let width = 512;
        let height = 512;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let sample_count = 1;

        let nannou_texture = wgpu::TextureBuilder::new()
            .size([width, height])
            .format(format)
            .sample_count(sample_count)
            .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING)
            .build(device);

        let view = nannou_texture.view().build();

        let renderer = nannou::draw::RendererBuilder::new().build(
            device,
            [width, height],
            1.0,
            sample_count,
            format,
        );

        self.gpu_state = Some(GpuState {
            device: device_ptr,
            queue: queue_ptr,
            renderer,
            nannou_texture,
            view,
            width,
            height,
        });
    }
}

export_plugin!(KameaPlugin);
