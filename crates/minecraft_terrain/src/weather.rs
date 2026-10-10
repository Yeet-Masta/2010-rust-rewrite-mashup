//! Rain and snow as the client draws them (26.3 `WeatherEffectRenderer`,
//! `ClientLevel.getPrecipitationAt` with `Biome.getPrecipitationAt`).
//!
//! Weather columns are in block space; the renderer draws them relative to
//! the eye like the mobs.

use crate::mesh::Vertex;
use crate::scene::{BlockPos, HandcraftedScene};
use minecraftoss_core::random::{LegacyRandom, RandomSource};
use minecraftoss_generator::temperature::BiomeTemperature;

/// `Biome.Precipitation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precipitation {
    None,
    Rain,
    Snow,
}

/// `Options.weatherRadius` at its default.
pub const WEATHER_RADIUS: i32 = 10;
/// The Overworld's `Level.getSeaLevel`.
const SEA_LEVEL: i32 = 63;
/// `ColumnInstance.uOffset` of snow is shifted by this much, so the shader
/// tells snow's texture from rain's.
pub const SNOW_U: f32 = 4.0;

/// The biomes' precipitation, with the noise that cools high places.
pub struct Climate {
    temperature: BiomeTemperature,
    /// `WeatherEffectRenderer.columnSizeX` and `columnSizeZ`: each column of
    /// the 32 by 32 table faces the camera's column.
    column_size: Vec<[f32; 2]>,
}

impl Default for Climate {
    fn default() -> Self {
        Self::new()
    }
}

/// One rain or snow column (`WeatherEffectRenderer.ColumnInstance`), with
/// its light (sky, block) and whether it snows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub x: i32,
    pub z: i32,
    pub bottom_y: i32,
    pub top_y: i32,
    pub u_offset: f32,
    pub v_offset: f32,
    pub light: (u8, u8),
    pub snow: bool,
}

impl Climate {
    pub fn new() -> Self {
        let mut column_size = vec![[0.0; 2]; 32 * 32];
        for z in 0..32 {
            for x in 0..32 {
                let (dx, dz) = (x as f32 - 16.0, z as f32 - 16.0);
                let distance = (dx * dx + dz * dz).sqrt();
                column_size[z * 32 + x] = [-dz / distance, dx / distance];
            }
        }
        Self { temperature: BiomeTemperature::new(), column_size }
    }

    /// `ClientLevel.getPrecipitationAt`: none where the chunk is not loaded,
    /// else the biome's (`Biome.getPrecipitationAt`: snow below 0.15, with
    /// the temperature falling above the snow line).
    pub fn precipitation_at(&self, scene: &HandcraftedScene, (x, y, z): BlockPos) -> Precipitation {
        if !scene.has_chunk((x.div_euclid(16), z.div_euclid(16))) {
            return Precipitation::None;
        }
        let (Some(states), Some(biome)) = (scene.states(), scene.biome_id_at((x, y, z))) else {
            return Precipitation::None;
        };
        let info = states.registries().biomes.get(biome);
        if !info.has_precipitation {
            Precipitation::None
        } else if self.temperature.at(info.temperature, info.frozen_temperature_modifier, x, y, z, SEA_LEVEL) >= 0.15 {
            Precipitation::Rain
        } else {
            Precipitation::Snow
        }
    }

    /// `WeatherEffectRenderer.extractRenderState`: a column for every
    /// block column within `radius` of the camera where something falls,
    /// from the motion-blocking surface (or `radius` below the camera) to
    /// `radius` above it. `light` gives (sky, block) light at a block.
    pub fn columns(
        &self,
        scene: &HandcraftedScene,
        light: impl Fn(BlockPos) -> (u8, u8),
        camera: [f64; 3],
        game_time: i64,
        partial_tick: f32,
        radius: i32,
    ) -> Vec<Column> {
        let [cx, cy, cz] = camera.map(|c| c.floor() as i32);
        let mut random = LegacyRandom::new(0);
        let mut out = Vec::new();
        for z in cz - radius..=cz + radius {
            for x in cx - radius..=cx + radius {
                let Some(terrain) = scene.motion_blocking_height(x, z) else { continue };
                let bottom = (cy - radius).max(terrain);
                let top = (cy + radius).max(terrain);
                if top == bottom {
                    continue;
                }
                let precipitation = self.precipitation_at(scene, (x, cy, z));
                if precipitation == Precipitation::None {
                    continue;
                }
                let seed = (x.wrapping_mul(x).wrapping_mul(3121).wrapping_add(x.wrapping_mul(45_238_971)))
                    ^ (z.wrapping_mul(z).wrapping_mul(418_711).wrapping_add(z.wrapping_mul(13761)));
                random.set_seed(i64::from(seed));
                let light = light((x, cy.max(terrain), z));
                let ticks = (game_time & 131_071) as i32;
                let column = if precipitation == Precipitation::Rain {
                    // `createRainColumnInstance`.
                    let offset = x
                        .wrapping_mul(x)
                        .wrapping_mul(3121)
                        .wrapping_add(x.wrapping_mul(45_238_971))
                        .wrapping_add(z.wrapping_mul(z).wrapping_mul(418_711))
                        .wrapping_add(z.wrapping_mul(13761))
                        & 0xFF;
                    let speed = 3.0 + random.next_f32();
                    let texture_offset = -((ticks + offset) as f32 + partial_tick) / 32.0 * speed;
                    Column { x, z, bottom_y: bottom, top_y: top, u_offset: 0.0, v_offset: texture_offset % 32.0, light, snow: false }
                } else {
                    // `createSnowColumnInstance`: drifting, a little brighter.
                    let time = ticks as f32 + partial_tick;
                    let u = (random.next_f64() + f64::from(time * 0.01 * random.next_gaussian() as f32)) as f32;
                    let v = (random.next_f64() + f64::from(time * random.next_gaussian() as f32 * 0.001)) as f32;
                    let v_offset = -((game_time & 511) as f32 + partial_tick) / 512.0;
                    let brighten = |level: u8| (u16::from(level) * 3 + 15) as u8 / 4;
                    Column { x, z, bottom_y: bottom, top_y: top, u_offset: u, v_offset: v_offset + v, light: (brighten(light.0), brighten(light.1)), snow: true }
                };
                out.push(column);
            }
        }
        out
    }

    /// `WeatherEffectRenderer.prepareInstances`: a quad per column, turned
    /// to face the camera's column, fading to half by the radius, at the
    /// rain's intensity (snow at most 0.8). Rain comes before snow; snow's
    /// texture U is shifted by [`SNOW_U`].
    pub fn column_mesh(&self, columns: &[Column], camera: [f64; 3], radius: i32, intensity: f32) -> (Vec<Vertex>, Vec<u32>) {
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        let radius_sq = (radius * radius) as f32;
        let (cx, cz) = (camera[0].floor() as i32, camera[2].floor() as i32);
        for snow in [false, true] {
            let max_alpha = if snow { 0.8 } else { 1.0 };
            for column in columns.iter().filter(|c| c.snow == snow) {
                let rel_x = (f64::from(column.x) + 0.5 - camera[0]) as f32;
                let rel_z = (f64::from(column.z) + 0.5 - camera[2]) as f32;
                let fade = ((rel_x * rel_x + rel_z * rel_z) / radius_sq).min(1.0);
                let alpha = (max_alpha + (0.5 - max_alpha) * fade) * intensity;
                let index = (column.z - cz + 16) * 32 + column.x - cx + 16;
                let Some(&[size_x, size_z]) = usize::try_from(index).ok().and_then(|i| self.column_size.get(i)) else { continue };
                // The camera's own column has no direction (vanilla's NaN quad).
                if !size_x.is_finite() || !size_z.is_finite() {
                    continue;
                }
                let (x, z) = (f64::from(column.x) + 0.5, f64::from(column.z) + 0.5);
                let (half_x, half_z) = (f64::from(size_x / 2.0), f64::from(size_z / 2.0));
                let (x0, x1, z0, z1) = ((x - half_x) as f32, (x + half_x) as f32, (z - half_z) as f32, (z + half_z) as f32);
                let (y0, y1) = (column.bottom_y as f32, column.top_y as f32);
                let u0 = if snow { column.u_offset.rem_euclid(1.0) + SNOW_U } else { column.u_offset };
                let u1 = u0 + 1.0;
                let v0 = column.bottom_y as f32 * 0.25 + column.v_offset;
                let v1 = column.top_y as f32 * 0.25 + column.v_offset;
                let (sky, block) = (f32::from(column.light.0), f32::from(column.light.1));
                let base = vertices.len() as u32;
                for (position, uv) in [([x0, y1, z0], [u0, v0]), ([x1, y1, z1], [u1, v0]), ([x1, y0, z1], [u1, v1]), ([x0, y0, z0], [u0, v1])] {
                    vertices.push(Vertex { position, uv, color: [1.0, 1.0, 1.0, alpha], sky_light: sky, block_light: block });
                }
                indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
        (vertices, indices)
    }
}
