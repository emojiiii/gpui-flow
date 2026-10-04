//! Opt-in surfaceless host adapted from official fast/layers/tests.rs Harness.
//! Uses exactly the production pipelines, shaders, atlas, and frame recorder.
use crate::WgpuAtlas;
use crate::fast::flow_namespace::{
    FlowExecutionReceipt, FlowFailureStage, FlowLayerNamespace, GpuErrors,
    with_flow_layer_namespace,
};
use crate::fast::frame::{FrameHost, FrameState, FrameTarget};
use crate::wgpu_renderer::{
    GammaParams, GlobalParams, RenderingParameters, WgpuBindGroupLayouts, WgpuPipelines,
    WgpuRenderer,
};
use anyhow::{Context as _, Result};
use gpui::{DevicePixels, Rgba, Scene, Size};
use std::num::NonZeroU64;
use std::sync::Arc;

/// A completed native GPU-backend frame, associated with its producer identity.
/// A CPU/software wgpu adapter still executes this path; consult adapter_info().
pub struct FlowRenderedFrame {
    pub namespace: FlowLayerNamespace,
    pub rgba: Vec<u8>,
    pub receipt: FlowExecutionReceipt,
}

const INITIAL_INSTANCE_CAPACITY: u64 = 2 * 1024 * 1024;

/// Draws scenes into textures without a surface, through the renderer's
/// pipelines and frame recording.
pub struct FlowHeadlessRenderer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    format: wgpu::TextureFormat,
    atlas: WgpuAtlas,
    pipelines: WgpuPipelines,
    bind_group_layouts: WgpuBindGroupLayouts,
    atlas_sampler: wgpu::Sampler,
    rendering_params: RenderingParameters,
    globals_buffer: wgpu::Buffer,
    path_globals_offset: u64,
    gamma_offset: u64,
    globals_bind_group: wgpu::BindGroup,
    path_globals_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    instance_alignment: u64,
    path_targets: Option<PathTargets>,
    state: FrameState,
    adapter_info: wgpu::AdapterInfo,
    output: Option<wgpu::Texture>,
}

/// The path intermediate texture, and its multisampled twin, of one size.
struct PathTargets {
    size: Size<DevicePixels>,
    _intermediate: wgpu::Texture,
    intermediate_view: wgpu::TextureView,
    _msaa: Option<wgpu::Texture>,
    msaa_view: Option<wgpu::TextureView>,
}

impl FlowHeadlessRenderer {
    /// Requests a real adapter/device; lack of an adapter is an error.
    pub fn new() -> Result<FlowHeadlessRenderer> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY | wgpu::Backends::GL,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });
        let adapter = gpui::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .context("no usable headless wgpu adapter")?;
        let (device, queue, dual_source_blending, color_texture_format) =
            gpui::block_on(crate::WgpuContext::create_device(&adapter))?;
        let errors = GpuErrors::new(&device, true);
        let (device, queue) = (Arc::new(device), Arc::new(queue));

        // The surface's choice when it offers both.
        let usages = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
        let format = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ]
        .into_iter()
        .find(|format| {
            adapter
                .get_texture_format_features(*format)
                .allowed_usages
                .contains(usages)
        })
        .context("adapter has no supported RGBA readback target format")?;

        let rendering_params = RenderingParameters::new(&adapter, format);
        let bind_group_layouts = WgpuRenderer::create_bind_group_layouts(&device, false);
        let pipelines = WgpuRenderer::create_pipelines(
            &device,
            &bind_group_layouts,
            format,
            wgpu::CompositeAlphaMode::Opaque,
            rendering_params.path_sample_count,
            dual_source_blending,
            false,
        );
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let uniform_alignment = device.limits().min_uniform_buffer_offset_alignment as u64;
        let globals_size = size_of::<GlobalParams>() as u64;
        let gamma_size = size_of::<GammaParams>() as u64;
        let path_globals_offset = globals_size.next_multiple_of(uniform_alignment);
        let gamma_offset = (path_globals_offset + globals_size).next_multiple_of(uniform_alignment);
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals_buffer"),
            size: gamma_offset + gamma_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind_group = |label: &str, offset: u64| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &bind_group_layouts.globals,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &globals_buffer,
                            offset,
                            size: NonZeroU64::new(globals_size),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &globals_buffer,
                            offset: gamma_offset,
                            size: NonZeroU64::new(gamma_size),
                        }),
                    },
                ],
            })
        };
        let path_globals_bind_group =
            globals_bind_group("path_globals_bind_group", path_globals_offset);
        let globals_bind_group = globals_bind_group("globals_bind_group", 0);

        let instance_alignment = device.limits().min_storage_buffer_offset_alignment as u64;
        let instance_buffer = create_instance_buffer(&device, INITIAL_INSTANCE_CAPACITY);
        let atlas = WgpuAtlas::new(device.clone(), queue.clone(), color_texture_format);

        errors.check().context("headless renderer initialization")?;
        Ok(FlowHeadlessRenderer {
            device,
            queue,
            format,
            atlas,
            pipelines,
            bind_group_layouts,
            atlas_sampler,
            rendering_params,
            globals_buffer,
            path_globals_offset,
            gamma_offset,
            globals_bind_group,
            path_globals_bind_group,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            instance_alignment,
            path_targets: None,
            state: FrameState::default(),
            adapter_info: adapter.get_info(),
            output: None,
        })
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }
    pub fn atlas(&self) -> &WgpuAtlas {
        &self.atlas
    }
    pub fn flow_execution_receipt(&self) -> Option<&FlowExecutionReceipt> {
        self.state.flow.receipt.as_ref()
    }
    pub fn flow_failure_stage(&self) -> Option<FlowFailureStage> {
        self.state.flow.failure.get()
    }

    /// Records through the production frame implementation, retaining the
    /// output target and GPU tile cache between calls. Returns tightly packed,
    /// top-to-bottom RGBA8 bytes only after successful readback completion.
    pub fn render(
        &mut self,
        namespace: FlowLayerNamespace,
        scene: &Scene,
        size: Size<DevicePixels>,
        clear: Rgba,
    ) -> Result<FlowRenderedFrame> {
        with_flow_layer_namespace(namespace, || {
            super::flow_namespace::begin_draw(&mut self.state);
            let result = self.render_inner(scene, size, clear);
            match result {
                Ok(rgba) => {
                    self.state.flow.readback_completed();
                    let receipt = self
                        .state
                        .flow
                        .receipt
                        .as_mut()
                        .context("frame did not produce a submission receipt")?;
                    Ok(FlowRenderedFrame {
                        namespace,
                        rgba,
                        receipt: *receipt,
                    })
                }
                Err(error) => {
                    let stage = self
                        .state
                        .flow
                        .failure
                        .get()
                        .unwrap_or(FlowFailureStage::Readback);
                    self.state.flow.fail(stage);
                    self.state.layers.clear();
                    self.state.bind_groups = Default::default();
                    Err(error)
                }
            }
        })
    }

    fn render_inner(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
        clear: Rgba,
    ) -> Result<Vec<u8>> {
        let max = self.device.limits().max_texture_dimension_2d;
        anyhow::ensure!(
            size.width.0 > 0 && size.height.0 > 0,
            "headless dimensions must be positive"
        );
        let (width, height) = (size.width.0 as u32, size.height.0 as u32);
        anyhow::ensure!(
            width <= max && height <= max,
            "headless dimensions exceed adapter limit {max}"
        );
        let errors = GpuErrors::new(&self.device, true);
        self.write_globals(width, height);
        self.ensure_path_targets(size);
        self.atlas.before_frame();
        if self
            .output
            .as_ref()
            .is_none_or(|texture| texture.width() != width || texture.height() != height)
        {
            self.state.layers.clear();
            self.state.bind_groups = Default::default();
            self.output = Some(self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("flow_headless_target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }));
        }
        let texture = self
            .output
            .as_ref()
            .context("missing headless target")?
            .clone();
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        errors.check().context("headless frame preparation")?;
        let clear = wgpu::Color {
            r: clear.r as f64,
            g: clear.g as f64,
            b: clear.b as f64,
            a: clear.a as f64,
        };
        crate::fast::frame::record_into(self, scene, &view, clear)?;
        // A submission receipt alone is not an oracle result. Hold it back
        // until the mapping has completed and all readback errors were checked.
        self.state
            .flow
            .failure
            .set(Some(FlowFailureStage::Readback));
        let pixels = read_back(&self.device, &self.queue, &texture, self.format)?;
        self.state.flow.failure.set(None);
        Ok(pixels)
    }

    fn write_globals(&self, width: u32, height: u32) {
        let globals = GlobalParams {
            viewport_size: [width as f32, height as f32],
            premultiplied_alpha: 0,
            pad: 0,
        };
        let gamma = GammaParams {
            gamma_ratios: self.rendering_params.gamma_ratios,
            grayscale_enhanced_contrast: self.rendering_params.grayscale_enhanced_contrast,
            subpixel_enhanced_contrast: self.rendering_params.subpixel_enhanced_contrast,
            is_bgr: 0,
            _pad: 0,
        };
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        self.queue.write_buffer(
            &self.globals_buffer,
            self.path_globals_offset,
            bytemuck::bytes_of(&globals),
        );
        self.queue.write_buffer(
            &self.globals_buffer,
            self.gamma_offset,
            bytemuck::bytes_of(&gamma),
        );
    }

    fn ensure_path_targets(&mut self, size: Size<DevicePixels>) {
        if self
            .path_targets
            .as_ref()
            .is_some_and(|targets| targets.size == size)
        {
            return;
        }
        let (width, height) = (size.width.0 as u32, size.height.0 as u32);
        let (intermediate, intermediate_view) =
            WgpuRenderer::create_path_intermediate(&self.device, self.format, width, height);
        let (msaa, msaa_view) = WgpuRenderer::create_msaa_if_needed(
            &self.device,
            self.format,
            width,
            height,
            self.rendering_params.path_sample_count,
        )
        .unzip();
        self.path_targets = Some(PathTargets {
            size,
            _intermediate: intermediate,
            intermediate_view,
            _msaa: msaa,
            msaa_view,
        });
    }
}

impl FrameHost for FlowHeadlessRenderer {
    fn frame_state(&mut self) -> &mut FrameState {
        &mut self.state
    }

    fn instance_data_alignment(&self) -> u64 {
        self.instance_alignment.max(1)
    }

    fn reserve_instance_data(&mut self, size: u64) -> Result<()> {
        if size > self.instance_capacity {
            let capacity = size
                .checked_next_power_of_two()
                .context("instance capacity overflow")?;
            anyhow::ensure!(
                capacity <= self.device.limits().max_buffer_size,
                "instance buffer exceeds device limit"
            );
            self.instance_capacity = capacity;
            self.instance_buffer = create_instance_buffer(&self.device, self.instance_capacity);
        }
        Ok(())
    }

    fn target(&self) -> Result<FrameTarget<'_>> {
        let path_targets = self.path_targets.as_ref();
        Ok(FrameTarget {
            device: &self.device,
            queue: &self.queue,
            pipelines: &self.pipelines,
            bind_group_layouts: &self.bind_group_layouts,
            atlas: &self.atlas,
            atlas_sampler: &self.atlas_sampler,
            globals_bind_group: &self.globals_bind_group,
            path_globals_bind_group: &self.path_globals_bind_group,
            path_intermediate_view: path_targets.map(|targets| &targets.intermediate_view),
            path_msaa_view: path_targets.and_then(|targets| targets.msaa_view.as_ref()),
            instance_buffer: &self.instance_buffer,
            globals_buffer: &self.globals_buffer,
            gamma_offset: self.gamma_offset,
            gamma_size: size_of::<GammaParams>() as u64,
            format: self.format,
            path_sample_count: self.rendering_params.path_sample_count,
            premultiplied_alpha: false,
            viewport_size: path_targets.map_or([1., 1.], |targets| {
                [targets.size.width.0 as f32, targets.size.height.0 as f32]
            }),
        })
    }
}

fn create_instance_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instance_buffer"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The pixels of `texture` as RGBA bytes, row by row.
fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    format: wgpu::TextureFormat,
) -> Result<Vec<u8>> {
    let errors = GpuErrors::new(device, true);
    let (width, height) = (texture.width(), texture.height());
    let row = width * 4;
    let padded_row = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("harness_readback"),
        size: padded_row as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("harness_readback"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let commands = encoder.finish();
    errors.check().context("headless readback encoding")?;
    let errors = GpuErrors::new(device, true);
    let submission = queue.submit(std::iter::once(commands));
    errors.check().context("headless readback submission")?;
    let errors = GpuErrors::new(device, true);
    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(30)),
        })
        .context("headless readback device poll")?;
    receiver
        .recv_timeout(std::time::Duration::from_secs(30))
        .context("headless readback callback timed out")?
        .context("headless readback mapping failed")?;
    errors.check().context("headless readback GPU validation")?;
    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity(row as usize * height as usize);
    for y in 0..height {
        let start = y as usize * padded_row as usize;
        pixels.extend_from_slice(&mapped[start..start + row as usize]);
    }
    drop(mapped);
    buffer.unmap();
    if format == wgpu::TextureFormat::Bgra8Unorm {
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
    }
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        AtlasTile, Bounds, ContentMask, Corners, Hsla, LayerFrame, LayerKey, Path,
        PolychromeSprite, Quad, ScaledPixels, TileCoord, layer_tile_id, layer_tile_texture_id,
        point, px, rgba, size,
    };

    fn scene(color: Rgba) -> Scene {
        let bounds = Bounds {
            origin: point(ScaledPixels(0.), ScaledPixels(0.)),
            size: size(ScaledPixels(16.), ScaledPixels(16.)),
        };
        let mut content = Scene::default();
        content.insert_primitive(Quad {
            bounds,
            content_mask: ContentMask { bounds },
            background: Hsla::from(color).into(),
            ..Default::default()
        });
        content.finish();
        let key = LayerKey(1);
        let tile = TileCoord { x: 0, y: 0 };
        let mut scene = Scene::default();
        scene.layers.frames.push(LayerFrame {
            key,
            generation: 1,
            background: color,
            tile_size: 16,
            content: content.into(),
            dirty_tiles: vec![tile],
        });
        scene.insert_primitive(PolychromeSprite {
            order: 0,
            pad: 0,
            grayscale: false.into(),
            opacity: 1.,
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: Corners::default(),
            tile: AtlasTile {
                texture_id: layer_tile_texture_id(key),
                tile_id: layer_tile_id(tile),
                padding: 0,
                bounds: Bounds {
                    origin: point(DevicePixels(0), DevicePixels(0)),
                    size: size(DevicePixels(16), DevicePixels(16)),
                },
            },
        });
        scene.finish();
        scene
    }

    /// These are the two original, unmodified solid triangles at a native
    /// Flow rounded rectangle's edge after fractional zoom. A tile-local
    /// projection covered four MSAA samples at (1024, 200), while the window
    /// projection covered three. Adapter absence is an error, not a skip.
    #[test]
    fn window_space_path_tiles_preserve_fractional_msaa_coverage() -> Result<()> {
        let mut renderer = FlowHeadlessRenderer::new()?;
        let dimensions = size(DevicePixels(1280), DevicePixels(640));
        let background = rgba(0x182028ff);
        let clip = Bounds::new(point(px(640.), px(0.)), size(px(640.), px(640.)));
        let mut path = Path::new(point(px(0.), px(0.)));
        // Binary fractions preserve the captured f32 values exactly.
        let a = point(px(262_565. / 256.), px(46_308. / 256.));
        let b = point(px(259_770. / 256.), px(46_308. / 256.));
        let c = point(px(262_565. / 256.), px(51_354. / 256.));
        let d = point(px(1017.), px(51_510. / 256.));
        let solid = (point(0., 1.), point(0., 1.), point(0., 1.));
        path.push_triangle((a, b, c), solid);
        path.push_triangle((c, b, d), solid);
        path.color = Hsla::from(rgba(0xf8aa2689)).into();
        path.content_mask = ContentMask { bounds: clip };
        let mut content = Scene::default();
        content.insert_primitive(path.scale(1.));
        content.finish();
        let direct =
            renderer.render(FlowLayerNamespace::new(), &content, dimensions, background)?;

        let key = LayerKey(1);
        let coords: Vec<_> = (0..2)
            .flat_map(|y| (0..3).map(move |x| TileCoord { x, y }))
            .collect();
        let mask = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(1280.), ScaledPixels(640.)),
        );
        let layer = LayerFrame {
            key,
            generation: 1,
            background,
            tile_size: 512,
            content: content.into(),
            dirty_tiles: coords.clone(),
        };
        let mut cached = Scene::default();
        for coord in coords {
            cached.insert_primitive(PolychromeSprite {
                order: 0,
                pad: 0,
                grayscale: false.into(),
                opacity: 1.,
                bounds: layer.tile_bounds(coord),
                content_mask: ContentMask { bounds: mask },
                corner_radii: Corners::default(),
                tile: AtlasTile {
                    texture_id: layer_tile_texture_id(key),
                    tile_id: layer_tile_id(coord),
                    padding: 0,
                    bounds: Bounds {
                        origin: point(DevicePixels(0), DevicePixels(0)),
                        size: size(DevicePixels(512), DevicePixels(512)),
                    },
                },
            });
        }
        cached.layers.frames.push(layer);
        cached.finish();
        let namespace = FlowLayerNamespace::new_window_space();
        let first = renderer.render(namespace, &cached, dimensions, background)?;
        assert!(first.receipt.tile_raster_passes > 0);
        assert_eq!(first.rgba.len(), direct.rgba.len());
        assert_eq!(
            first
                .rgba
                .iter()
                .zip(&direct.rgba)
                .position(|(a, b)| a != b),
            None,
            "window-space tiles differ from direct RGBA at this byte offset",
        );
        let warm = renderer.render(namespace, &cached, dimensions, background)?;
        assert_eq!(warm.receipt.tile_raster_passes, 0);
        assert!(warm.rgba == direct.rgba, "warm window-space RGBA changed");
        Ok(())
    }

    /// Assert the inclusive pixel-center clip contract independently of a
    /// tiled/reference comparison that would exercise the same shader twice.
    #[test]
    fn path_fragment_clip_includes_an_exact_half_pixel_edge() -> Result<()> {
        let mut renderer = FlowHeadlessRenderer::new()?;
        let rectangle = |left: f32, right: f32, clip_right: f32, color: Rgba| {
            let mut path = Path::new(point(px(0.), px(0.)));
            let a = point(px(left), px(100.));
            let b = point(px(right), px(100.));
            let c = point(px(left), px(200.));
            let d = point(px(right), px(200.));
            let solid = (point(0., 1.), point(0., 1.), point(0., 1.));
            path.push_triangle((a, b, c), solid);
            path.push_triangle((b, d, c), solid);
            path.color = Hsla::from(color).into();
            path.content_mask = ContentMask {
                bounds: Bounds::new(point(px(0.), px(0.)), size(px(clip_right), px(640.))),
            };
            path.scale(1.)
        };
        let mut content = Scene::default();
        content.insert_primitive(rectangle(300., 400., 331.5, rgba(0xff0000ff)));
        // Its bounds include the origin, so it shares one path batch at a
        // later draw order. The union sprite extends past the tested clip;
        // otherwise sprite-edge coverage could hide the fragment clip result.
        content.insert_primitive(rectangle(400., 420., 420., rgba(0)));
        content.finish();
        assert_ne!(content.paths[0].order, content.paths[1].order);
        let output = renderer.render(
            FlowLayerNamespace::new(),
            &content,
            size(DevicePixels(1280), DevicePixels(640)),
            rgba(0x182028ff),
        )?;
        let at_edge = (150 * 1280 + 331) * 4;
        let outside = at_edge + 4;
        assert_eq!(&output.rgba[at_edge..at_edge + 4], &[255, 0, 0, 255]);
        assert_eq!(&output.rgba[outside..outside + 4], &[24, 32, 40, 255]);
        Ok(())
    }

    /// No adapter is a hard failure, never a successful skipped oracle.
    #[test]
    fn flow_gpu_readback_receipt_and_namespace_isolation() -> Result<()> {
        let mut renderer = FlowHeadlessRenderer::new()?;
        let namespace = FlowLayerNamespace::new();
        let dimensions = size(DevicePixels(16), DevicePixels(16));
        let red = scene(rgba(0xff0000ff));
        let first = renderer.render(namespace, &red, dimensions, rgba(0x000000ff))?;
        assert_eq!(first.namespace, namespace);
        assert_eq!(first.rgba.len(), 16 * 16 * 4);
        assert!(
            first
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [255, 0, 0, 255])
        );
        assert_eq!(first.receipt.tile_raster_passes, 1);
        assert_eq!(first.receipt.instance_upload_calls, 1);
        assert!(first.receipt.instance_upload_bytes > 0);
        assert!(
            first.receipt.encoded && first.receipt.submitted && first.receipt.readback_completed
        );
        let output_id = renderer.output.clone();
        let cached = renderer.render(namespace, &red, dimensions, rgba(0x000000ff))?;
        assert_eq!(
            output_id, renderer.output,
            "the GPU target persists between frames"
        );
        assert_eq!(cached.rgba, first.rgba);
        assert_eq!(cached.receipt.tile_raster_passes, 0);
        assert_eq!(
            cached.receipt.instance_upload_calls, 1,
            "composition still uploads instances"
        );
        assert!(cached.receipt.instance_upload_bytes < first.receipt.instance_upload_bytes);
        let blue = renderer.render(
            FlowLayerNamespace::new(),
            &scene(rgba(0x0000ffff)),
            dimensions,
            rgba(0x000000ff),
        )?;
        assert_eq!(
            blue.receipt.tile_raster_passes, 1,
            "same numeric layer key must not alias"
        );
        assert!(
            blue.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0, 0, 255, 255])
        );
        assert!(
            renderer
                .render(
                    namespace,
                    &red,
                    size(DevicePixels(0), DevicePixels(16)),
                    rgba(0)
                )
                .is_err()
        );
        assert!(renderer.flow_execution_receipt().is_none());
        assert_eq!(
            renderer.flow_failure_stage(),
            Some(FlowFailureStage::Encoding)
        );
        let retry = renderer.render(namespace, &red, dimensions, rgba(0x000000ff))?;
        assert_eq!(retry.receipt.tile_raster_passes, 1);
        assert_eq!(retry.rgba, first.rgba);
        // Encoding is valid; destroying an input afterwards makes the actual
        // Queue::submit fail. Exercise the same checked submission as frames,
        // rather than fabricating a failure flag or trusting an old receipt.
        let input = renderer.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("destroyed_submit_input"),
            size: 4,
            usage: wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let output = renderer.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("failed_submit_output"),
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&input, 0, &output, 0, 4);
        let commands = encoder.finish();
        input.destroy();
        let (result, receipt) = crate::with_flow_layer_namespace_receipt(namespace, || {
            crate::fast::flow_namespace::begin_draw(&mut renderer.state);
            renderer
                .state
                .flow
                .submit(&renderer.device, &renderer.queue, commands)
        });
        assert!(
            result.is_err(),
            "destroyed input must reject the actual submit"
        );
        assert!(receipt.is_none());
        assert!(renderer.flow_execution_receipt().is_none());
        assert_eq!(
            renderer.flow_failure_stage(),
            Some(FlowFailureStage::Submission)
        );
        let invalid_source = renderer.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("invalid_readback_source"),
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: renderer.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT, // Deliberately lacks COPY_SRC.
            view_formats: &[],
        });
        assert!(
            read_back(
                &renderer.device,
                &renderer.queue,
                &invalid_source,
                renderer.format
            )
            .is_err()
        );
        Ok(())
    }
}
