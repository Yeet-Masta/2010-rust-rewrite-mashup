//! The weather on the client (26.3 `ClientLevel`): the rain and thunder
//! levels the server sends, rain splashing on the ground with its sound
//! (`tickWeatherEffects`), lightning's flash (`skyFlashTime`) and thunder
//! (`LightningBolt.tick` on the client), and the rain, snow and bolt meshes
//! (`WeatherEffectRenderer`, `LightningBoltRenderer`).
use std::collections::HashSet;

use glam::DVec3;
use minecraft_terrain::fluid::{FluidCell, FluidKind};
use minecraft_terrain::mesh::ChunkMesh;
use minecraft_terrain::scene::Scene;
use minecraft_terrain::server::ServerLightning;
use minecraft_terrain::weather::{Climate, Precipitation, WEATHER_RADIUS, lightning_mesh};
use minecraftoss_core::random::LegacyRandom;
use minecraftoss_player::World as _;

use crate::particles::{Particles, Type};
use crate::world::World;

/// A sound to play: event, position, volume, pitch.
type Sound = (&'static str, DVec3, f32, f32);

pub struct Weather {
    climate: Climate,
    /// `Level.rainLevel` and `thunderLevel` as the server last sent them
    /// (the client sets them without easing).
    pub rain_level: f32,
    thunder_level: f32,
    /// `ClientLevel.skyFlashTime`.
    sky_flash_time: i32,
    /// `ClientLevel.rainSoundTime`.
    rain_sound_time: i32,
    /// The bolts already heard, by entity ID.
    heard: HashSet<i32>,
    /// Stands in for the bolts' own randoms (their thunder's pitch).
    random: LegacyRandom,
}

impl Weather {
    pub fn new() -> Self {
        Self {
            climate: Climate::new(),
            rain_level: 0.0,
            thunder_level: 0.0,
            sky_flash_time: 0,
            rain_sound_time: 0,
            heard: HashSet::new(),
            random: LegacyRandom::new(0x5DEE_CE66),
        }
    }

    /// The levels the server sent (`RAIN_LEVEL_CHANGE`,
    /// `THUNDER_LEVEL_CHANGE`).
    pub fn set_levels(&mut self, rain: f32, thunder: f32) {
        self.rain_level = rain.clamp(0.0, 1.0);
        self.thunder_level = thunder.clamp(0.0, 1.0);
    }

    /// `Level.getThunderLevel`: scaled by the rain.
    pub fn thunder(&self) -> f32 {
        self.thunder_level * self.rain_level
    }

    /// Lightning lights the sky (`skyFlashTime > 0`).
    pub fn flashing(&self) -> bool {
        self.sky_flash_time > 0
    }

    /// One client tick: `ClientLevel.tick`'s `tickWeatherEffects` and the
    /// flash running down, then the bolts' own ticks: thunder and impact
    /// when one appears (heard from afar), the flash while one shines.
    pub fn tick(
        &mut self,
        world: &World,
        particles: &mut Particles,
        camera: DVec3,
        game_time: i64,
        bolts: &[ServerLightning],
        sounds: &mut Vec<Sound>,
    ) {
        self.tick_rain(world, particles, camera, game_time, sounds);
        if self.sky_flash_time > 0 {
            self.sky_flash_time -= 1;
        }
        self.heard.retain(|id| bolts.iter().any(|b| b.id == *id));
        for bolt in bolts {
            let at = DVec3::from_array(bolt.position);
            if self.heard.insert(bolt.id) {
                let pitch = 0.8 + self.random.next_f32() * 0.2;
                sounds.push(("minecraft:entity.lightning_bolt.thunder", at, 10000.0, pitch));
                let pitch = 0.5 + self.random.next_f32() * 0.2;
                sounds.push(("minecraft:entity.lightning_bolt.impact", at, 2.0, pitch));
            }
            if bolt.life >= 0 {
                self.sky_flash_time = 2;
            }
        }
    }

    /// `ClientLevel.tickWeatherEffects`: rain splashes (smoke on lava,
    /// magma and lit campfires) on the surface within ten blocks up or down
    /// of the camera, and now and then the rain's sound at one of them,
    /// muffled when the camera is under cover below it.
    fn tick_rain(&mut self, world: &World, particles: &mut Particles, camera: DVec3, game_time: i64, sounds: &mut Vec<Sound>) {
        let rain = self.rain_level;
        if rain <= 0.0 {
            return;
        }
        let scene = &world.scene;
        let mut random = LegacyRandom::new(game_time.wrapping_mul(312_987_231));
        let [cx, cy, cz] = camera.to_array().map(|c| c.floor() as i32);
        let diameter = 2 * WEATHER_RADIUS + 1;
        let count = (0.225 * (diameter * diameter) as f32 * rain * rain) as i32;
        let min_y = Scene::vertical_range(scene).start;
        let mut splashed = None;
        for _ in 0..count {
            let x = cx + random.next_i32_bound(diameter) - WEATHER_RADIUS;
            let z = cz + random.next_i32_bound(diameter) - WEATHER_RADIUS;
            let Some(top) = scene.motion_blocking_height(x, z) else { continue };
            if top <= min_y || top > cy + 10 || top < cy - 10 || self.climate.precipitation_at(scene, (x, top, z)) != Precipitation::Rain {
                continue;
            }
            let pos = (x, top - 1, z);
            splashed = Some(pos);
            let (bx, bz) = (random.next_f64(), random.next_f64());
            // `VoxelShape.max(Y, x, z)`: the shape's top over that point.
            let block_top = scene
                .collision_boxes(pos)
                .iter()
                .filter(|b| b[0] <= bx && bx <= b[3] && b[2] <= bz && bz <= b[5])
                .map(|b| b[4])
                .fold(f64::NEG_INFINITY, f64::max);
            let fluid = FluidCell::at(scene, pos);
            let fluid_top = fluid.map_or(0.0, |f| f64::from(f.height(scene, pos)));
            let block = world.block(pos);
            let id = block.map(|b| b.id.key());
            let lit_campfire = block.is_some_and(|b| {
                b.id.path.ends_with("campfire") && b.properties.get("lit").is_some_and(|l| l.as_str() == "true")
            });
            let smoke = fluid.is_some_and(|f| f.kind == FluidKind::Lava) || id.as_deref() == Some("minecraft:magma_block") || lit_campfire;
            let kind = if smoke { Type::Smoke } else { Type::Rain };
            let at = DVec3::new(f64::from(x) + bx, f64::from(top - 1) + block_top.max(fluid_top), f64::from(z) + bz);
            particles.spawn(world, &kind.into(), at, DVec3::ZERO);
        }
        let Some(pos) = splashed else { return };
        let last = self.rain_sound_time;
        self.rain_sound_time += 1;
        if random.next_i32_bound(3) < last {
            self.rain_sound_time = 0;
            let at = DVec3::new(f64::from(pos.0) + 0.5, f64::from(pos.1) + 0.5, f64::from(pos.2) + 0.5);
            let covered = scene.motion_blocking_height(cx, cz).is_some_and(|h| h > cy);
            if pos.1 > cy + 1 && covered {
                sounds.push(("minecraft:weather.rain.above", at, 0.1, 0.5));
            } else {
                sounds.push(("minecraft:weather.rain", at, 0.2, 1.0));
            }
        }
    }

    /// `WeatherEffectRenderer.extractRenderState` and `prepare`: this
    /// frame's rain and snow columns around the camera.
    pub fn mesh(&self, world: &World, camera: DVec3, game_time: i64, partial_tick: f32) -> ChunkMesh {
        if self.rain_level <= 0.0 {
            return ChunkMesh::default();
        }
        let light = |pos| (world.light.get(pos), world.light.get_block(pos));
        let columns = self.climate.columns(&world.scene, light, camera.to_array(), game_time, partial_tick, WEATHER_RADIUS);
        let (vertices, indices) = self.climate.column_mesh(&columns, camera.to_array(), WEATHER_RADIUS, self.rain_level);
        ChunkMesh { vertices, indices, ..ChunkMesh::default() }
    }

    /// `LightningBoltRenderer`: every bolt there is.
    pub fn bolt_mesh(bolts: &[ServerLightning]) -> (Vec<[f32; 7]>, Vec<u32>) {
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        for bolt in bolts {
            lightning_mesh(bolt.seed, bolt.position, &mut vertices, &mut indices);
        }
        (vertices, indices)
    }
}
