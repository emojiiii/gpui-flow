//! Opt-in surfaceless host using the production GPU renderer core.
use crate::fast::flow_namespace::{
    FlowExecutionReceipt, FlowFailureStage, FlowLayerNamespace, GpuErrors,
    with_flow_layer_namespace,
};
use crate::wgpu_renderer::WgpuRendererCore;
use crate::{WgpuAtlas, WgpuContext};
use anyhow::{Context as _, Result};
use gpui::{DevicePixels, Rgba, Scene, Size};
use std::sync::Arc;

/// A completed native GPU-backend frame, associated with its producer identity.
/// A CPU/software wgpu adapter still executes this path; consult adapter_info().
pub struct FlowRenderedFrame {
    pub namespace: FlowLayerNamespace,
    pub rgba: Vec<u8>,
    pub receipt: FlowExecutionReceipt,
}

/// Draws scenes into textures without a surface, through the renderer's
/// pipelines and frame recording.
pub struct FlowHeadlessRenderer {
    core: WgpuRendererCore,
    adapter_info: wgpu::AdapterInfo,
    output: Option<wgpu::Texture>,
}

impl FlowHeadlessRenderer {
    /// Requests a real adapter/device; lack of an adapter is an error.
    pub fn new() -> Result<Self> {
        let (context, format) = WgpuContext::create_headless()?;
        let errors = GpuErrors::new(&context.device, true);
        let atlas = Arc::new(WgpuAtlas::from_context(&context));
        let core = WgpuRendererCore::new(&context, atlas, format, wgpu::CompositeAlphaMode::Opaque);
        errors.check().context("headless renderer initialization")?;
        Ok(Self {
            core,
            adapter_info: context.adapter.get_info(),
            output: None,
        })
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    pub fn atlas(&self) -> &WgpuAtlas {
        &self.core.atlas
    }

    pub fn flow_execution_receipt(&self) -> Option<&FlowExecutionReceipt> {
        self.core.fast_frame.flow.receipt.as_ref()
    }

    pub fn flow_failure_stage(&self) -> Option<FlowFailureStage> {
        self.core.fast_frame.flow.failure.get()
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
            super::flow_namespace::begin_draw(Some(&mut self.core.fast_frame));
            let result = self.render_inner(scene, size, clear);
            let state = &mut self.core.fast_frame;
            match result {
                Ok(rgba) => {
                    state.flow.readback_completed();
                    let receipt = state
                        .flow
                        .receipt
                        .context("frame did not produce a submission receipt")?;
                    Ok(FlowRenderedFrame {
                        namespace,
                        rgba,
                        receipt,
                    })
                }
                Err(error) => {
                    let stage = state
                        .flow
                        .failure
                        .get()
                        .unwrap_or(FlowFailureStage::Readback);
                    state.flow.fail(stage);
                    state.layers.clear();
                    state.bind_groups = Default::default();
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
        let device = &self.core.resources.device;
        let max = device.limits().max_texture_dimension_2d;
        anyhow::ensure!(
            size.width.0 > 0 && size.height.0 > 0,
            "headless dimensions must be positive"
        );
        let (width, height) = (size.width.0 as u32, size.height.0 as u32);
        anyhow::ensure!(
            width <= max && height <= max,
            "headless dimensions exceed adapter limit {max}"
        );
        let errors = GpuErrors::new(device, true);
        if self
            .output
            .as_ref()
            .is_none_or(|texture| texture.width() != width || texture.height() != height)
        {
            self.core.fast_frame.layers.clear();
            self.core.fast_frame.bind_groups = Default::default();
            self.output = Some(device.create_texture(&wgpu::TextureDescriptor {
                label: Some("flow_headless_target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.core.target_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }));
        }
        let texture = self.output.as_ref().expect("headless target just prepared");
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        errors.check().context("headless frame preparation")?;
        let clear = wgpu::Color {
            r: clear.r as f64,
            g: clear.g as f64,
            b: clear.b as f64,
            a: clear.a as f64,
        };
        self.core.render_frame(scene, &view, size, clear)?;

        // A submission receipt alone is not an oracle result. Hold it back
        // until the mapping has completed and all readback errors were checked.
        let flow = &self.core.fast_frame.flow;
        flow.failure.set(Some(FlowFailureStage::Readback));
        let pixels = read_back(
            &self.core.resources.device,
            &self.core.resources.queue,
            texture,
            self.core.target_format,
        )?;
        flow.failure.set(None);
        Ok(pixels)
    }
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
    let mapped = slice.get_mapped_range()?;
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
        AtlasTile, Bounds, ContentMask, Corners, Hsla, LayerFrame, LayerKey, PaintedPath,
        PathBuilder, PolychromeSprite, Quad, ScaledPixels, TileCoord, layer_tile_id,
        layer_tile_texture_id, point, px, rgba, size,
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

    /// Replays a captured Flow rounded rectangle edge after fractional zoom.
    /// It crosses a tile boundary and must match the direct analytical path.
    #[test]
    fn window_space_path_tiles_match_direct_at_a_fractional_edge() -> Result<()> {
        let mut renderer = FlowHeadlessRenderer::new()?;
        let dimensions = size(DevicePixels(1280), DevicePixels(640));
        let background = rgba(0x182028ff);
        let clip = Bounds::new(point(px(640.), px(0.)), size(px(640.), px(640.)));
        let mut path = PathBuilder::fill();
        // Binary fractions preserve the captured f32 values exactly.
        let a = point(px(262_565. / 256.), px(46_308. / 256.));
        let b = point(px(259_770. / 256.), px(46_308. / 256.));
        let c = point(px(262_565. / 256.), px(51_354. / 256.));
        let d = point(px(1017.), px(51_510. / 256.));
        path.move_to(a);
        path.line_to(b);
        path.line_to(d);
        path.line_to(c);
        path.close();
        let mut content = Scene::default();
        content.insert_primitive(PaintedPath::new(
            path.build()?,
            1.,
            ContentMask { bounds: clip },
            Hsla::from(rgba(0xf8aa2689)).into(),
        ));
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
    fn path_clip_includes_an_exact_half_pixel_edge() -> Result<()> {
        let mut renderer = FlowHeadlessRenderer::new()?;
        let mut path = PathBuilder::fill();
        path.move_to(point(px(300.), px(100.)));
        path.line_to(point(px(400.), px(100.)));
        path.line_to(point(px(400.), px(200.)));
        path.line_to(point(px(300.), px(200.)));
        path.close();
        let mut content = Scene::default();
        content.insert_primitive(PaintedPath::new(
            path.build()?,
            1.,
            ContentMask {
                bounds: Bounds::new(point(px(0.), px(0.)), size(px(331.5), px(640.))),
            },
            Hsla::from(rgba(0xff0000ff)).into(),
        ));
        content.finish();
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
        let device = &renderer.core.resources.device;
        let queue = &renderer.core.resources.queue;
        let format = renderer.core.target_format;
        let input = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("destroyed_submit_input"),
            size: 4,
            usage: wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("failed_submit_output"),
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&input, 0, &output, 0, 4);
        let commands = encoder.finish();
        input.destroy();
        let (result, receipt) = crate::with_flow_layer_namespace_receipt(namespace, || {
            let state = &mut renderer.core.fast_frame;
            crate::fast::flow_namespace::begin_draw(Some(state));
            state.flow.submit(device, queue, commands)
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
        let invalid_source = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("invalid_readback_source"),
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT, // Deliberately lacks COPY_SRC.
            view_formats: &[],
        });
        assert!(read_back(device, queue, &invalid_source, format).is_err());
        Ok(())
    }
}
