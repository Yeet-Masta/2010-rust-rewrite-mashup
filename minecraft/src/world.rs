//! A MinecraftOSS world: the integrated server's chunk map streams a seeded
//! world around the player, chunks are lit and meshed on worker threads, and
//! the Overworld's clock drives its sky, fog and light.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use glam::Vec3;
use minecraft_terrain::clouds::CloudMask;
use minecraft_terrain::day_cycle::{DayCycle, Skybox};
use minecraft_terrain::environment::{DimensionEnvironment, View};
use minecraft_terrain::lighting::SkyLight;
use minecraft_terrain::mesh::Atlas;
use minecraft_terrain::pack::{PackStack, ResourceId};
use minecraft_terrain::scene::{Block, BlockPos, HandcraftedScene, Scene};
use minecraft_terrain::sections::CullCamera;
use minecraft_terrain::terrain::{Dimension, FrameUpdate, TerrainStream};
use minecraftoss_core::registries::{DataPaths, Registries};

const TICK_SECONDS: f64 = 1.0 / 20.0;
/// Chunk sections fade in over this long, as the viewer's default option.
const FADE_MILLIS: u64 = 750;

pub struct World {
    pub stream: TerrainStream,
    pub scene: HandcraftedScene,
    pub packs: PackStack,
    pub atlas: Arc<Atlas>,
    pub registries: Arc<Registries>,
    pub seed: i64,
    pub view_distance: i32,
    /// The client JAR, for loot tables and recipes.
    pub jar: Option<PathBuf>,
    /// Item stack sizes, durability and attributes.
    pub item_catalog: Option<PathBuf>,
    pub celestial: image::RgbaImage,
    pub crack_texture: image::RgbaImage,
    pub light: SkyLight,
    pub day: DayCycle,
    environment: DimensionEnvironment,
    environment_clock: f64,
    environment_primed: bool,
    cloud_mask: Option<CloudMask>,
    cloud_center: Option<(i32, i32)>,
}

impl World {
    /// Generates the world of `seed` around its spawn; with `save`, its
    /// chunks load from and are stored in that folder.
    pub fn load(
        root: &Path,
        seed: i64,
        view_distance: i32,
        save: Option<&Path>,
    ) -> Result<Self, String> {
        let paths = DataPaths::under(root);
        let registries = Arc::new(Registries::load(&paths)?);
        let packs = PackStack::open(vec![root.join(crate::setup::RESOURCE_PACK)])
            .map_err(|e| e.to_string())?;
        let stream = TerrainStream::for_dimension(
            registries.clone(),
            seed,
            view_distance,
            Dimension::Overworld,
            save,
        )
        .map_err(|e| e.to_string())?;
        let build = minecraft_terrain::mesh::build(&HandcraftedScene::default(), &packs)
            .map_err(|e| e.to_string())?;
        let scene = HandcraftedScene::streamed(stream.states.clone());
        let environment =
            DimensionEnvironment::load(&registries, Dimension::Overworld.dimension_type())?;
        let celestial = celestial_image(&packs).map_err(|e| e.to_string())?;
        let crack_texture = crack_strip(&packs).map_err(|e| e.to_string())?;
        let cloud_mask = CloudMask::from_pack(&packs).ok();
        let jar = Some(root.join(crate::setup::CLIENT_JAR)).filter(|jar| jar.is_file());
        let item_catalog =
            Some(root.join("artifacts/item-catalog/26.3.json")).filter(|path| path.is_file());
        Ok(Self {
            stream,
            scene,
            packs,
            atlas: build.atlas,
            registries,
            seed,
            view_distance,
            jar,
            item_catalog,
            celestial,
            crack_texture,
            light: SkyLight::streamed(),
            day: DayCycle::default(),
            environment,
            environment_clock: 0.0,
            environment_primed: false,
            cloud_mask,
            cloud_center: None,
        })
    }

    pub fn block(&self, pos: BlockPos) -> Option<&Block> {
        Scene::block(&self.scene, pos)
    }

    /// Whether the chunk holding a block has been generated and sent.
    pub fn chunk_ready(&self, (x, _, z): BlockPos) -> bool {
        self.scene.generated_chunk((x >> 4, z >> 4)).is_some()
    }

    /// Sets blocks the player or a mob changed: the scene shows them, the
    /// chunk map saves them, and the sections and light around rebuild.
    pub fn set_blocks(&mut self, changes: &[(BlockPos, Option<Block>)]) {
        if changes.is_empty() {
            return;
        }
        let positions: Vec<BlockPos> = changes.iter().map(|(pos, _)| *pos).collect();
        for (pos, block) in changes {
            self.scene.set(*pos, block.clone());
        }
        self.stream.record_edits(&self.scene, &positions);
        self.stream.mark_edited(&self.scene, &positions);
    }

    /// The block's collision boxes in block-local units.
    pub fn collision_boxes(&self, pos: BlockPos) -> Vec<[f64; 6]> {
        self.scene.state_at(pos).map_or_else(Vec::new, |state| {
            self.registries.blocks.collision_boxes(state)
        })
    }

    /// One frame of terrain work around the camera: section meshes to
    /// upload and the world light updated.
    pub fn frame(&mut self, camera: &CullCamera) -> FrameUpdate {
        let mut update =
            self.stream
                .frame(&self.scene, camera, FADE_MILLIS, &self.atlas, &self.packs);
        for (chunk, column) in std::mem::take(&mut update.lights) {
            self.light.set_chunk_column(chunk, column);
        }
        update
    }

    /// `gameplay/sky_light_level` now, for mob burning and sky darkening.
    pub fn sky_light_level(&self) -> f32 {
        self.environment.sky_light_level()
    }

    /// Advances the clock and the environment attributes, and returns the
    /// viewer's environment uniform for a camera at `eye` looking along
    /// `forward`.
    pub fn environment(
        &mut self,
        dt: f64,
        eye: [f64; 3],
        forward: Vec3,
        aspect: f32,
    ) -> [[f32; 4]; 16] {
        self.day.advance(dt);
        let eye_block = (
            eye[0].floor() as i32,
            eye[1].floor() as i32,
            eye[2].floor() as i32,
        );
        self.environment
            .update_rain_fog(0.0, self.light.get(eye_block), false, (dt * 20.0) as f32);
        self.environment_clock += dt;
        if !self.environment_primed || self.environment_clock >= TICK_SECONDS {
            self.environment_clock = (self.environment_clock % TICK_SECONDS).min(TICK_SECONDS);
            let scene = &self.scene;
            self.environment.tick(
                self.day.ticks.floor() as i64,
                0.0,
                0.0,
                eye,
                |x, y, z| scene.noise_biome((x, y, z)).map_or(0, |id| id.0),
                !self.environment_primed,
            );
            self.environment_primed = true;
        }
        let partial_tick = (self.environment_clock / TICK_SECONDS).clamp(0.0, 1.0) as f32;
        let sky = self.environment.sky_state(&View {
            partial_tick,
            forward,
            camera_y: eye[1] as f32,
            render_distance: self.view_distance as u32,
            rain_level: 0.0,
            thunder_level: 0.0,
        });
        let render_distance = self.view_distance as f32 * 16.0;
        let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(forward).normalize_or(Vec3::Y);
        let put = |v: Vec3| [v.x, v.y, v.z, 0.0];
        let game_time = self.day.ticks;
        [
            put(forward),
            put(right),
            put(up),
            [eye[0] as f32, eye[1] as f32, eye[2] as f32, 0.0],
            put(sky.sky),
            [
                sky.fog.x,
                sky.fog.y,
                sky.fog.z,
                render_distance.min(sky.sky_fog_end),
            ],
            [
                sky.sky_light_color.x,
                sky.sky_light_color.y,
                sky.sky_light_color.z,
                sky.sky_light_factor,
            ],
            sky.sunset,
            [
                sky.sun_direction.x,
                sky.sun_direction.y,
                sky.sun_direction.z,
                sky.rain_brightness,
            ],
            [
                sky.moon_direction.x,
                sky.moon_direction.y,
                sky.moon_direction.z,
                sky.rain_brightness,
            ],
            // The brightness option at its default.
            [sky.cloud.x, sky.cloud.y, sky.cloud.z, 0.5],
            [aspect, 0.0, sky.star_brightness, sky.star_angle],
            [
                sky.moon_phase as f32,
                (game_time as f32) * 0.03,
                96.0,
                160.0,
            ],
            [
                sky.fog_start,
                sky.fog_end,
                render_distance - (render_distance / 10.0).clamp(4.0, 64.0),
                render_distance,
            ],
            [
                sky.ambient.x,
                sky.ambient.y,
                sky.ambient.z,
                match sky.skybox {
                    Skybox::Overworld => 0.0,
                    Skybox::End => 1.0,
                    _ => 2.0,
                },
            ],
            [
                sky.block_light_tint.x,
                sky.block_light_tint.y,
                sky.block_light_tint.z,
                sky.block_factor,
            ],
        ]
    }

    /// Cloud geometry, rebuilt when the eye crosses a cloud cell: position
    /// then colour per vertex.
    pub fn clouds(&mut self, eye: [f64; 3]) -> Option<(Vec<[f32; 7]>, Vec<u32>)> {
        let mask = self.cloud_mask.as_ref()?;
        let center = mask.center(eye[0] as f32, eye[2] as f32, self.day.ticks);
        if self.cloud_center == Some(center) {
            return None;
        }
        self.cloud_center = Some(center);
        let mesh = mask.build(center, eye[1] as f32);
        let vertices = mesh
            .vertices
            .iter()
            .map(|v| {
                let (p, c) = (v.position, v.color);
                [p[0], p[1], p[2], c[0], c[1], c[2], c[3]]
            })
            .collect();
        Some((vertices, mesh.indices))
    }
}

/// The sky's sun and moon phases, side by side as the sky shader reads them.
fn celestial_image(packs: &PackStack) -> anyhow::Result<image::RgbaImage> {
    let mut celestial = image::RgbaImage::new(32 * 9, 32);
    let names = [
        "environment/celestial/sun",
        "environment/celestial/moon/full_moon",
        "environment/celestial/moon/waning_gibbous",
        "environment/celestial/moon/third_quarter",
        "environment/celestial/moon/waning_crescent",
        "environment/celestial/moon/new_moon",
        "environment/celestial/moon/waxing_crescent",
        "environment/celestial/moon/first_quarter",
        "environment/celestial/moon/waxing_gibbous",
    ];
    for (index, path) in names.iter().enumerate() {
        let id = ResourceId::parse(&format!("minecraft:{path}"))?;
        if let Some(bytes) = packs.texture(&id)? {
            let img =
                image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)?.to_rgba8();
            let tile = image::imageops::resize(&img, 32, 32, image::imageops::FilterType::Nearest);
            image::imageops::replace(&mut celestial, &tile, (index as i64) * 32, 0);
        }
    }
    Ok(celestial)
}

/// The ten destroy stages side by side, 16 pixels each.
fn crack_strip(packs: &PackStack) -> anyhow::Result<image::RgbaImage> {
    let mut strip = image::RgbaImage::new(16 * 10, 16);
    for stage in 0..10u32 {
        let id = ResourceId::parse(&format!("minecraft:block/destroy_stage_{stage}"))?;
        if let Some(bytes) = packs.texture(&id)? {
            let img =
                image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)?.to_rgba8();
            let tile = image::imageops::resize(&img, 16, 16, image::imageops::FilterType::Nearest);
            image::imageops::replace(&mut strip, &tile, i64::from(stage) * 16, 0);
        }
    }
    Ok(strip)
}
