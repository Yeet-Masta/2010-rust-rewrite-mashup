//! The window's pixels: MinecraftOSS section meshes, sky, clouds, mobs,
//! particles and the hand through the viewer's shaders, then the HUD on top.
//! Everything 3D is in block space, drawn relative to the eye.
use std::collections::HashMap;
use std::sync::Arc;

use glam::Mat4;
use minecraft_terrain::mesh::{Atlas, ChunkMesh, SectionMesh, SectionVertex, Vertex};
use minecraft_terrain::sections::SectionPos;
use wgpu::util::DeviceExt;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SECTION_VERTEX_BYTES: u64 = std::mem::size_of::<SectionVertex>() as u64;
const ENTITY_VERTEX_BYTES: u64 = std::mem::size_of::<Vertex>() as u64;

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewUniform {
    clip_from_rel: [f32; 16],
    rel_from_clip: [f32; 16],
    hand_clip: [f32; 16],
    camera: [f32; 4],
    environment: [[f32; 4]; 16],
}

/// One 2D vertex: window pixels, texture coordinates, colour.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct UiVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub colour: [f32; 4],
}

/// A texture the HUD draws from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(usize);

/// White, for flat fills.
pub const WHITE: TextureId = TextureId(0);

/// A run of quads from one texture.
#[derive(Clone, Copy, Debug)]
pub struct UiBatch {
    pub texture: TextureId,
    /// Inverts what is behind it, as the crosshair does.
    pub invert: bool,
    pub start: u32,
    pub count: u32,
}

/// The 2D layer of a frame.
#[derive(Default)]
pub struct UiList {
    pub vertices: Vec<UiVertex>,
    pub batches: Vec<UiBatch>,
}

impl UiList {
    /// A rectangle in window pixels with its texture rectangle.
    pub fn quad(&mut self, texture: TextureId, rect: [f32; 4], uv: [f32; 4], colour: [f32; 4]) {
        self.push(texture, false, rect, uv, colour);
    }

    pub fn inverted(&mut self, texture: TextureId, rect: [f32; 4], uv: [f32; 4]) {
        self.push(texture, true, rect, uv, [1.0; 4]);
    }

    pub fn fill(&mut self, rect: [f32; 4], colour: [f32; 4]) {
        self.push(WHITE, false, rect, [0.0, 0.0, 1.0, 1.0], colour);
    }

    fn push(
        &mut self,
        texture: TextureId,
        invert: bool,
        [x, y, w, h]: [f32; 4],
        [u0, v0, u1, v1]: [f32; 4],
        colour: [f32; 4],
    ) {
        let start = self.vertices.len() as u32;
        let v = |px: f32, py: f32, u: f32, t: f32| UiVertex {
            position: [px, py],
            uv: [u, t],
            colour,
        };
        self.vertices.extend_from_slice(&[
            v(x, y, u0, v0),
            v(x + w, y, u1, v0),
            v(x + w, y + h, u1, v1),
            v(x, y, u0, v0),
            v(x + w, y + h, u1, v1),
            v(x, y + h, u0, v1),
        ]);
        match self.batches.last_mut() {
            Some(batch)
                if batch.texture == texture
                    && batch.invert == invert
                    && batch.start + batch.count == start =>
            {
                batch.count += 6
            }
            _ => self.batches.push(UiBatch {
                texture,
                invert,
                start,
                count: 6,
            }),
        }
    }
}

/// What the world looks like this frame, beyond its sections.
#[derive(Default)]
pub struct WorldDraw {
    /// Projection times the view's rotation: eye-relative blocks to clip.
    pub clip_from_rel: Mat4,
    /// The eye in blocks.
    pub eye: [f32; 3],
    pub environment: [[f32; 4]; 16],
    /// Sections to draw, nearest first.
    pub visible: Vec<SectionPos>,
    /// Break particles and item entities, as section vertices.
    pub particles: (Vec<SectionVertex>, Vec<u32>),
    /// The particle engine's quads: cut out, and blended.
    pub sprites: crate::particles::ParticleMesh,
    /// Mob models (cut out, back-face culled, translucent) and shadows.
    pub entities: [ChunkMesh; 4],
    /// Destroy stage cubes: position then strip uv.
    pub cracks: (Vec<[f32; 5]>, Vec<u32>),
    /// The targeted block's outline, as line segments.
    pub outline: Vec<[f32; 3]>,
    /// The hand or held item in view space, and its projection.
    pub hand: (Vec<SectionVertex>, Vec<u32>),
    pub hand_clip: Mat4,
}

struct Mesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
}

struct SectionGpu {
    mesh: Mesh,
    transparent_start: u32,
}

struct UiTexture {
    texture: wgpu::Texture,
    bind: wgpu::BindGroup,
    size: (u32, u32),
}

struct AtlasGpu {
    atlas: Arc<Atlas>,
    texture: wgpu::Texture,
    /// The animation frame each animated tile last showed.
    shown: Vec<Option<(usize, usize, u16)>>,
}

struct Pipelines {
    sky: wgpu::RenderPipeline,
    opaque: wgpu::RenderPipeline,
    translucent: wgpu::RenderPipeline,
    particle: wgpu::RenderPipeline,
    particle_translucent: wgpu::RenderPipeline,
    clouds: wgpu::RenderPipeline,
    crack: wgpu::RenderPipeline,
    entity: wgpu::RenderPipeline,
    entity_culled: wgpu::RenderPipeline,
    entity_translucent: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    hand: wgpu::RenderPipeline,
    outline: wgpu::RenderPipeline,
    ui: wgpu::RenderPipeline,
    ui_invert: wgpu::RenderPipeline,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// The window has no area (minimised): nothing is drawn.
    minimized: bool,
    /// The format drawn in: the surface's, without sRGB encoding, since
    /// Minecraft's colours are display values.
    format: wgpu::TextureFormat,
    depth: wgpu::TextureView,
    pipelines: Pipelines,
    world_layout: wgpu::BindGroupLayout,
    ui_texture_layout: wgpu::BindGroupLayout,
    view_buffer: wgpu::Buffer,
    screen_buffer: wgpu::Buffer,
    screen_bind: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    ui_sampler: wgpu::Sampler,
    atlas: Option<AtlasGpu>,
    celestial: Option<wgpu::TextureView>,
    cracks: Option<wgpu::TextureView>,
    world_bind: Option<wgpu::BindGroup>,
    sections: HashMap<SectionPos, SectionGpu>,
    clouds: Option<Mesh>,
    textures: Vec<UiTexture>,
}

impl Renderer {
    /// The renderer for a window; `vsync` shows one frame per display
    /// refresh.
    pub fn new(window: Arc<winit::window::Window>, vsync: bool) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone())),
        );
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))?;
        log!(
            "Graphics: {} ({:?})",
            adapter.get_info().name,
            adapter.get_info().backend
        );
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("minecraft"),
                required_features: wgpu::Features::empty(),
                required_limits:
                    wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                ..Default::default()
            }))?;
        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| anyhow::anyhow!("the window's surface offers no formats"))?;
        let format = surface_format.remove_srgb_suffix();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: if vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            desired_maximum_frame_latency: 2,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: if format == surface_format {
                vec![]
            } else {
                vec![format]
            },
        };
        surface.configure(&device, &config);
        let depth = depth_view(&device, config.width, config.height);

        let world_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(3),
                texture_entry(4),
            ],
        });
        let screen_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let ui_texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui texture"),
            entries: &[
                texture_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipelines = Pipelines::new(
            &device,
            format,
            &world_layout,
            &screen_layout,
            &ui_texture_layout,
        );
        let view_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view"),
            size: std::mem::size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let screen_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let screen_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("screen"),
            layout: &screen_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("world"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let ui_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ui"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let mut renderer = Self {
            surface,
            device,
            queue,
            config,
            minimized: false,
            format,
            depth,
            pipelines,
            world_layout,
            ui_texture_layout,
            view_buffer,
            screen_buffer,
            screen_bind,
            sampler,
            ui_sampler,
            atlas: None,
            celestial: None,
            cracks: None,
            world_bind: None,
            sections: HashMap::new(),
            clouds: None,
            textures: Vec::new(),
        };
        let white = image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]));
        renderer.add_texture(&white);
        Ok(renderer)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.minimized = width == 0 || height == 0;
        if self.minimized {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = depth_view(&self.device, width, height);
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// A texture for the HUD.
    pub fn add_texture(&mut self, image: &image::RgbaImage) -> TextureId {
        let (width, height) = image.dimensions();
        let texture = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("ui"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            image.as_raw(),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui"),
            layout: &self.ui_texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.ui_sampler),
                },
            ],
        });
        self.textures.push(UiTexture {
            texture,
            bind,
            size: (width, height),
        });
        TextureId(self.textures.len() - 1)
    }

    /// Replaces a rectangle of a HUD texture's pixels.
    pub fn update_region(&mut self, id: TextureId, x: u32, y: u32, image: &image::RgbaImage) {
        let Some(held) = self.textures.get(id.0) else {
            return;
        };
        let (width, height) = image.dimensions();
        if x + width > held.size.0 || y + height > held.size.1 {
            return;
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &held.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            image.as_raw(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// A world's textures: the block and entity atlas with its mipmaps, the
    /// sun and moon, and the destroy stages. Drops the last world's sections.
    pub fn set_world(
        &mut self,
        atlas: Arc<Atlas>,
        celestial: &image::RgbaImage,
        cracks: &image::RgbaImage,
    ) {
        let mut levels = vec![&atlas.pixels];
        levels.extend(atlas.mipmaps.iter());
        let (width, height) = atlas.pixels.dimensions();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, image) in levels.iter().enumerate() {
            let (w, h) = image.dimensions();
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                image.as_raw(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
        let atlas_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let celestial = self.plain_texture(celestial);
        let cracks = self.plain_texture(cracks);
        self.world_bind = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world"),
            layout: &self.world_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.view_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&celestial),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&cracks),
                },
            ],
        }));
        self.atlas = Some(AtlasGpu {
            shown: vec![None; atlas.animated_tiles.len()],
            atlas,
            texture,
        });
        self.celestial = Some(celestial);
        self.cracks = Some(cracks);
        self.sections.clear();
        self.clouds = None;
    }

    fn plain_texture(&self, image: &image::RgbaImage) -> wgpu::TextureView {
        let (width, height) = image.dimensions();
        self.device
            .create_texture_with_data(
                &self.queue,
                &wgpu::TextureDescriptor {
                    label: Some("world image"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                image.as_raw(),
            )
            .create_view(&wgpu::TextureViewDescriptor::default())
    }

    /// Water, lava, fire and the other animated textures at a game tick
    /// (milliseconds of game time drive interpolated ones smoothly).
    pub fn animate(&mut self, ticks: u64) {
        let Some(atlas) = self.atlas.as_mut() else {
            return;
        };
        for (index, tile) in atlas.atlas.animated_tiles.iter().enumerate() {
            if tile.sequence.is_empty() || tile.frames.is_empty() {
                continue;
            }
            let state = tile.state_at(ticks);
            // Interpolated tiles step their blend in eighths.
            let blend = if tile.interpolate {
                state.progress_millis / 125
            } else {
                0
            };
            let key = (state.current, state.next, blend);
            if atlas.shown[index] == Some(key) {
                continue;
            }
            atlas.shown[index] = Some(key);
            let state = minecraft_terrain::mesh::AnimationFrameState {
                progress_millis: blend * 125,
                ..state
            };
            for level in 0..tile.frames[state.current].len() {
                let image = tile.image_at(state, level);
                let (w, h) = image.dimensions();
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &atlas.texture,
                        mip_level: level as u32,
                        origin: wgpu::Origin3d {
                            x: tile.origin.0 >> level,
                            y: tile.origin.1 >> level,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    image.as_raw(),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(w * 4),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }

    /// New, rebuilt and dropped section meshes.
    pub fn update_sections(
        &mut self,
        uploads: Vec<(SectionPos, SectionMesh)>,
        removed: Vec<SectionPos>,
    ) {
        for pos in removed {
            self.sections.remove(&pos);
        }
        for (pos, mesh) in uploads {
            match upload(
                &self.device,
                bytemuck::cast_slice(&mesh.vertices),
                &mesh.indices,
            ) {
                Some(gpu) => {
                    let transparent_start =
                        mesh.transparent_start.unwrap_or(gpu.count).min(gpu.count);
                    self.sections.insert(
                        pos,
                        SectionGpu {
                            mesh: gpu,
                            transparent_start,
                        },
                    );
                }
                None => {
                    self.sections.remove(&pos);
                }
            }
        }
    }

    pub fn set_clouds(&mut self, vertices: &[[f32; 7]], indices: &[u32]) {
        self.clouds = upload(&self.device, bytemuck::cast_slice(vertices), indices);
    }

    /// Draws a frame and shows it; with `capture`, also saves it as a PNG.
    pub fn render(
        &mut self,
        world: Option<&WorldDraw>,
        ui: &UiList,
        capture: Option<&std::path::Path>,
    ) {
        if self.minimized {
            return;
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.format),
            ..Default::default()
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        let offscreen = capture.map(|_| self.offscreen());
        let target = offscreen.as_ref().map_or(&view, |(_, view)| view);
        self.encode(&mut encoder, target, world, ui);
        let readback = offscreen
            .as_ref()
            .map(|(texture, _)| self.copy_out(&mut encoder, texture));
        if offscreen.is_some() {
            // The shown frame too, so the window keeps updating.
            self.encode(&mut encoder, &view, world, ui);
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
        if let (Some(path), Some((buffer, padded))) = (capture, readback) {
            if let Err(error) = self.save(&buffer, padded, path) {
                log!("Screenshot failed: {error}");
            } else {
                log!("Saved {}", path.display());
            }
        }
    }

    fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        world: Option<&WorldDraw>,
        ui: &UiList,
    ) {
        let (width, height) = (self.config.width as f32, self.config.height as f32);
        self.queue.write_buffer(
            &self.screen_buffer,
            0,
            bytemuck::cast_slice(&[width, height, 0.0, 0.0]),
        );
        let world = world.filter(|_| self.world_bind.is_some());
        let mut frame_meshes = Vec::new();
        if let Some(world) = world {
            let uniform = ViewUniform {
                clip_from_rel: world.clip_from_rel.to_cols_array(),
                rel_from_clip: world.clip_from_rel.inverse().to_cols_array(),
                hand_clip: world.hand_clip.to_cols_array(),
                camera: [world.eye[0], world.eye[1], world.eye[2], 0.0],
                environment: world.environment,
            };
            self.queue
                .write_buffer(&self.view_buffer, 0, bytemuck::bytes_of(&uniform));
            let entity = |mesh: &ChunkMesh| {
                upload(
                    &self.device,
                    bytemuck::cast_slice(&mesh.vertices),
                    &mesh.indices,
                )
            };
            frame_meshes = vec![
                upload(
                    &self.device,
                    bytemuck::cast_slice(&world.particles.0),
                    &world.particles.1,
                ),
                entity(&world.entities[0]),
                entity(&world.entities[1]),
                entity(&world.entities[2]),
                entity(&world.entities[3]),
                upload(
                    &self.device,
                    bytemuck::cast_slice(&world.cracks.0),
                    &world.cracks.1,
                ),
                upload(
                    &self.device,
                    bytemuck::cast_slice(&world.hand.0),
                    &world.hand.1,
                ),
                upload(
                    &self.device,
                    bytemuck::cast_slice(&world.sprites.opaque.0),
                    &world.sprites.opaque.1,
                ),
                upload(
                    &self.device,
                    bytemuck::cast_slice(&world.sprites.translucent.0),
                    &world.sprites.translucent.1,
                ),
            ];
        }
        let outline = world.filter(|w| !w.outline.is_empty()).map(|w| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("outline"),
                    contents: bytemuck::cast_slice(&w.outline),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let ui_buffer = (!ui.vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("ui"),
                    contents: bytemuck::cast_slice(&ui.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let (Some(world), Some(bind)) = (world, self.world_bind.as_ref()) {
                let p = &self.pipelines;
                pass.set_bind_group(0, bind, &[]);
                pass.set_pipeline(&p.opaque);
                for pos in &world.visible {
                    let Some(section) = self.sections.get(pos) else {
                        continue;
                    };
                    if section.transparent_start == 0 {
                        continue;
                    }
                    draw(&mut pass, &section.mesh, 0..section.transparent_start);
                }
                if let Some(mesh) = &frame_meshes[0] {
                    draw(&mut pass, mesh, 0..mesh.count);
                }
                for (pipeline, mesh) in [
                    (&p.entity, &frame_meshes[1]),
                    (&p.entity_culled, &frame_meshes[2]),
                    (&p.shadow, &frame_meshes[4]),
                    (&p.particle, &frame_meshes[7]),
                ] {
                    if let Some(mesh) = mesh {
                        pass.set_pipeline(pipeline);
                        draw(&mut pass, mesh, 0..mesh.count);
                    }
                }
                // Sky wherever nothing has been drawn yet.
                pass.set_pipeline(&p.sky);
                pass.draw(0..3, 0..1);
                pass.set_pipeline(&p.translucent);
                for pos in world.visible.iter().rev() {
                    let Some(section) = self.sections.get(pos) else {
                        continue;
                    };
                    if section.transparent_start >= section.mesh.count {
                        continue;
                    }
                    draw(
                        &mut pass,
                        &section.mesh,
                        section.transparent_start..section.mesh.count,
                    );
                }
                if let Some(mesh) = &frame_meshes[3] {
                    pass.set_pipeline(&p.entity_translucent);
                    draw(&mut pass, mesh, 0..mesh.count);
                }
                if let Some(mesh) = &frame_meshes[8] {
                    pass.set_pipeline(&p.particle_translucent);
                    draw(&mut pass, mesh, 0..mesh.count);
                }
                if let Some(mesh) = &frame_meshes[5] {
                    pass.set_pipeline(&p.crack);
                    draw(&mut pass, mesh, 0..mesh.count);
                }
                if let (Some(buffer), Some(world)) = (outline.as_ref(), Some(world)) {
                    pass.set_pipeline(&p.outline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..world.outline.len() as u32, 0..1);
                }
                if let Some(clouds) = &self.clouds {
                    pass.set_pipeline(&p.clouds);
                    draw(&mut pass, clouds, 0..clouds.count);
                }
            }
        }
        // The hand over everything in the world, in a depth range of its own.
        if let (Some(mesh), Some(bind)) = (
            frame_meshes.get(6).and_then(Option::as_ref),
            self.world_bind.as_ref(),
        ) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hand"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, bind, &[]);
            pass.set_pipeline(&self.pipelines.hand);
            draw(&mut pass, mesh, 0..mesh.count);
        }
        if let Some(buffer) = ui_buffer {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.screen_bind, &[]);
            pass.set_vertex_buffer(0, buffer.slice(..));
            for batch in &ui.batches {
                let Some(texture) = self.textures.get(batch.texture.0) else {
                    continue;
                };
                pass.set_pipeline(if batch.invert {
                    &self.pipelines.ui_invert
                } else {
                    &self.pipelines.ui
                });
                pass.set_bind_group(1, &texture.bind, &[]);
                pass.draw(batch.start..batch.start + batch.count, 0..1);
            }
        }
    }

    fn offscreen(&self) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    fn copy_out(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> (wgpu::Buffer, u32) {
        let (width, height) = (self.config.width, self.config.height);
        let padded = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
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
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        (buffer, padded)
    }

    fn save(
        &self,
        buffer: &wgpu::Buffer,
        padded: u32,
        path: &std::path::Path,
    ) -> anyhow::Result<()> {
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })?;
        let data = slice.get_mapped_range();
        let (width, height) = (self.config.width, self.config.height);
        let bgra = matches!(self.format, wgpu::TextureFormat::Bgra8Unorm);
        let mut image = image::RgbaImage::new(width, height);
        for y in 0..height {
            let row = &data[(y * padded) as usize..][..(width * 4) as usize];
            for x in 0..width {
                let p = &row[(x * 4) as usize..][..4];
                let pixel = if bgra {
                    [p[2], p[1], p[0], 255]
                } else {
                    [p[0], p[1], p[2], 255]
                };
                image.put_pixel(x, y, image::Rgba(pixel));
            }
        }
        drop(data);
        buffer.unmap();
        image.save(path)?;
        Ok(())
    }
}

fn draw(pass: &mut wgpu::RenderPass<'_>, mesh: &Mesh, range: std::ops::Range<u32>) {
    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(range, 0, 0..1);
}

fn upload(device: &wgpu::Device, vertices: &[u8], indices: &[u32]) -> Option<Mesh> {
    if vertices.is_empty() || indices.is_empty() {
        return None;
    }
    Some(Mesh {
        vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vertices"),
            contents: vertices,
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("indices"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        }),
        count: indices.len() as u32,
    })
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

impl Pipelines {
    fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        world_layout: &wgpu::BindGroupLayout,
        screen_layout: &wgpu::BindGroupLayout,
        ui_texture_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let ui_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ui.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world"),
            bind_group_layouts: &[Some(world_layout)],
            immediate_size: 0,
        });
        let ui_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui"),
            bind_group_layouts: &[Some(screen_layout), Some(ui_texture_layout)],
            immediate_size: 0,
        });
        let section_attributes =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Unorm8x4, 3 => Unorm8x2];
        let section = [wgpu::VertexBufferLayout {
            array_stride: SECTION_VERTEX_BYTES,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &section_attributes,
        }];
        let cloud_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let cloud = [wgpu::VertexBufferLayout {
            array_stride: 28,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &cloud_attributes,
        }];
        let crack_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];
        let crack = [wgpu::VertexBufferLayout {
            array_stride: 20,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &crack_attributes,
        }];
        let entity_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4, 3 => Float32, 4 => Float32];
        let entity = [wgpu::VertexBufferLayout {
            array_stride: ENTITY_VERTEX_BYTES,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &entity_attributes,
        }];
        let outline_attributes = wgpu::vertex_attr_array![0 => Float32x3];
        let outline = [wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &outline_attributes,
        }];
        let ui_attributes =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
        let ui = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<UiVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ui_attributes,
        }];
        let crumbling = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Dst,
                dst_factor: wgpu::BlendFactor::Src,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let invert = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::OneMinusDst,
                dst_factor: wgpu::BlendFactor::OneMinusSrc,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        struct Spec<'a> {
            module: &'a wgpu::ShaderModule,
            layout: &'a wgpu::PipelineLayout,
            vertex: &'a str,
            fragment: &'a str,
            buffers: &'a [wgpu::VertexBufferLayout<'a>],
            /// Depth write and compare, or no depth at all.
            depth: Option<(bool, wgpu::CompareFunction)>,
            blend: Option<wgpu::BlendState>,
            cull: Option<wgpu::Face>,
            topology: wgpu::PrimitiveTopology,
        }
        let make = |spec: Spec<'_>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(spec.fragment),
                layout: Some(spec.layout),
                vertex: wgpu::VertexState {
                    module: spec.module,
                    entry_point: Some(spec.vertex),
                    compilation_options: Default::default(),
                    buffers: spec.buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: spec.module,
                    entry_point: Some(spec.fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: spec.blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: spec.topology,
                    cull_mode: spec.cull,
                    ..Default::default()
                },
                depth_stencil: spec.depth.map(|(write, compare)| wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(write),
                    depth_compare: Some(compare),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let world = |vertex, fragment, buffers, depth, blend, cull| {
            make(Spec {
                module: &shader,
                layout: &layout,
                vertex,
                fragment,
                buffers,
                depth,
                blend,
                cull,
                topology: wgpu::PrimitiveTopology::TriangleList,
            })
        };
        use wgpu::CompareFunction::{Equal, GreaterEqual};
        let alpha = Some(wgpu::BlendState::ALPHA_BLENDING);
        Self {
            sky: world(
                "sky_vertex",
                "sky_fragment",
                &[],
                Some((false, Equal)),
                None,
                None,
            ),
            opaque: world(
                "vertex",
                "opaque",
                &section,
                Some((true, GreaterEqual)),
                None,
                None,
            ),
            translucent: world(
                "vertex",
                "translucent",
                &section,
                Some((false, GreaterEqual)),
                alpha,
                None,
            ),
            // `OPAQUE_PARTICLE` and `TRANSLUCENT_PARTICLE`: both test and
            // write depth; the translucent ones blend.
            particle: world(
                "vertex",
                "particle",
                &section,
                Some((true, GreaterEqual)),
                None,
                None,
            ),
            particle_translucent: world(
                "vertex",
                "particle",
                &section,
                Some((true, GreaterEqual)),
                alpha,
                None,
            ),
            clouds: world(
                "cloud_vertex",
                "cloud_fragment",
                &cloud,
                Some((false, GreaterEqual)),
                alpha,
                None,
            ),
            crack: world(
                "crack_vertex",
                "crack_fragment",
                &crack,
                Some((false, GreaterEqual)),
                Some(crumbling),
                None,
            ),
            entity: world(
                "entity_vertex",
                "entity_fragment",
                &entity,
                Some((true, GreaterEqual)),
                None,
                None,
            ),
            entity_culled: world(
                "entity_vertex",
                "entity_fragment",
                &entity,
                Some((true, GreaterEqual)),
                None,
                Some(wgpu::Face::Back),
            ),
            entity_translucent: world(
                "entity_vertex",
                "entity_translucent_fragment",
                &entity,
                Some((false, GreaterEqual)),
                alpha,
                None,
            ),
            shadow: world(
                "entity_vertex",
                "shadow_fragment",
                &entity,
                Some((false, GreaterEqual)),
                alpha,
                None,
            ),
            hand: world(
                "hand_vertex",
                "hand_fragment",
                &section,
                Some((true, GreaterEqual)),
                None,
                None,
            ),
            outline: make(Spec {
                module: &shader,
                layout: &layout,
                vertex: "outline_vertex",
                fragment: "outline_fragment",
                buffers: &outline,
                depth: Some((false, GreaterEqual)),
                blend: alpha,
                cull: None,
                topology: wgpu::PrimitiveTopology::LineList,
            }),
            ui: make(Spec {
                module: &ui_shader,
                layout: &ui_layout,
                vertex: "vertex",
                fragment: "fragment",
                buffers: &ui,
                depth: None,
                blend: alpha,
                cull: None,
                topology: wgpu::PrimitiveTopology::TriangleList,
            }),
            ui_invert: make(Spec {
                module: &ui_shader,
                layout: &ui_layout,
                vertex: "vertex",
                fragment: "invert",
                buffers: &ui,
                depth: None,
                blend: Some(invert),
                cull: None,
                topology: wgpu::PrimitiveTopology::TriangleList,
            }),
        }
    }
}
