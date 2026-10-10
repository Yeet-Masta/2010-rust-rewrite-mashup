//! What the blocks around the player do on their own, as 26.3's client
//! level animates them: `ClientLevel.animateTick` samples 1334 positions a
//! tick within 16 and 32 blocks and runs each block's and fluid's
//! `animateTick` (torch flames, falling leaves, lava pops, drips through
//! ceilings, ambient sounds), and campfires and spawners tick their own
//! particles as block entities do.
use std::collections::HashSet;

use glam::DVec3;
use minecraft_terrain::scene::{Block, BlockPos};

use crate::particles::{Options, Particles, Random, Type};
use crate::world::World;

/// A sound an ambient block played: event, where, volume, pitch.
pub type Sound = (&'static str, DVec3, f32, f32);

/// The block entities that tick their own particles, found as the sampling
/// passes over them (`CampfireBlockEntity.particleTick`,
/// `BaseSpawner.clientTick`).
#[derive(Default)]
pub struct Ambient {
    campfires: HashSet<BlockPos>,
    spawners: HashSet<BlockPos>,
}

/// The block state queries `animateTick` needs, over the client's blocks.
struct Level<'a> {
    world: &'a World,
}

impl Level<'_> {
    fn block(&self, pos: BlockPos) -> Option<&Block> {
        self.world.block(pos)
    }

    fn path(&self, pos: BlockPos) -> &str {
        self.block(pos).map_or("air", |b| b.id.path.as_str())
    }

    /// `BlockState.is(tag)` (air where the world has no block).
    fn in_tag(&self, pos: BlockPos, tag: &str) -> bool {
        let Some(states) = self.world.scene.states() else {
            return false;
        };
        let registries = states.registries();
        let name = self.block(pos).map_or_else(
            || "minecraft:air".to_owned(),
            |block| format!("{}:{}", block.id.namespace, block.id.path),
        );
        let (Ok(tag), Some(block)) = (
            registries.block_tags.require(tag),
            registries.blocks.block_by_name(&name),
        ) else {
            return false;
        };
        registries.block_tags.contains(tag, usize::from(block.0))
    }

    fn is_air(&self, pos: BlockPos) -> bool {
        matches!(self.path(pos), "air" | "cave_air" | "void_air")
    }

    fn prop(&self, pos: BlockPos, key: &str) -> Option<&str> {
        self.block(pos)?.properties.get(key).map(String::as_str)
    }

    fn boxes(&self, pos: BlockPos) -> Vec<[f64; 6]> {
        self.world.collision_boxes(pos)
    }

    /// `isCollisionShapeFullBlock`.
    fn full_block(&self, pos: BlockPos) -> bool {
        let boxes = self.boxes(pos);
        boxes.len() == 1 && boxes[0] == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    }

    /// `isFaceSturdy(face)` for the bottom (`up == false`) or top face: a
    /// box covering the whole face.
    fn face_sturdy(&self, pos: BlockPos, up: bool) -> bool {
        self.boxes(pos).iter().any(|b| {
            b[0] <= 0.0
                && b[2] <= 0.0
                && b[3] >= 1.0
                && b[5] >= 1.0
                && if up { b[4] >= 1.0 } else { b[1] <= 0.0 }
        })
    }

    /// The fluid at a block: water (with source and falling) or lava.
    fn fluid(&self, pos: BlockPos) -> Option<Fluid> {
        let block = self.block(pos)?;
        let level = block
            .properties
            .get("level")
            .and_then(|l| l.parse::<u32>().ok())
            .unwrap_or(0);
        match block.id.path.as_str() {
            "water" => Some(Fluid {
                lava: false,
                source: level == 0,
                falling: level >= 8,
            }),
            "lava" => Some(Fluid {
                lava: true,
                source: level == 0,
                falling: level >= 8,
            }),
            "bubble_column" | "kelp" | "kelp_plant" | "seagrass" | "tall_seagrass" => Some(Fluid {
                lava: false,
                source: true,
                falling: false,
            }),
            _ if block
                .properties
                .get("waterlogged")
                .is_some_and(|w| w == "true") =>
            {
                Some(Fluid {
                    lava: false,
                    source: true,
                    falling: false,
                })
            }
            _ => None,
        }
    }

    /// `isSolidRender`: an opaque full cube.
    fn solid_render(&self, pos: BlockPos) -> bool {
        let path = self.path(pos);
        self.full_block(pos)
            && ![
                "glass", "leaves", "ice", "slime", "honey", "spawner", "barrier",
            ]
            .iter()
            .any(|word| path.contains(word))
    }
}

#[derive(Clone, Copy)]
struct Fluid {
    lava: bool,
    source: bool,
    falling: bool,
}

fn center(pos: BlockPos) -> DVec3 {
    DVec3::new(
        f64::from(pos.0) + 0.5,
        f64::from(pos.1) + 0.5,
        f64::from(pos.2) + 0.5,
    )
}

fn corner(pos: BlockPos) -> DVec3 {
    DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2))
}

fn offset(pos: BlockPos, d: (i32, i32, i32)) -> BlockPos {
    (pos.0 + d.0, pos.1 + d.1, pos.2 + d.2)
}

/// A horizontal facing's unit step.
fn step(facing: &str) -> (i32, i32, i32) {
    match facing {
        "north" => (0, 0, -1),
        "south" => (0, 0, 1),
        "west" => (-1, 0, 0),
        "east" => (1, 0, 0),
        "up" => (0, 1, 0),
        "down" => (0, -1, 0),
        _ => (0, 0, 0),
    }
}

const ALL: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];
const HORIZONTAL: [(i32, i32, i32); 4] = [(0, 0, -1), (1, 0, 0), (0, 0, 1), (-1, 0, 0)];

/// `Mth.getSeed`.
fn position_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut s = (i64::from(x).wrapping_mul(3_129_871))
        ^ (i64::from(z).wrapping_mul(116_129_781))
        ^ i64::from(y);
    s = s
        .wrapping_mul(s)
        .wrapping_mul(42_317_861)
        .wrapping_add(s.wrapping_mul(11));
    s >> 16
}

struct Emit<'a> {
    level: Level<'a>,
    particles: &'a mut Particles,
    sounds: &'a mut Vec<Sound>,
    /// The time of day, for night-only sounds.
    day_time: f64,
}

impl Emit<'_> {
    fn spawn(&mut self, options: impl Into<Options>, at: DVec3, motion: DVec3) {
        let world = self.level.world;
        self.particles.spawn(world, &options.into(), at, motion);
    }

    fn sound(&mut self, event: &'static str, at: DVec3, volume: f32, pitch: f32) {
        self.sounds.push((event, at, volume, pitch));
    }

    fn rng(&mut self) -> &mut Random {
        &mut self.particles.random
    }

    fn rf(&mut self) -> f32 {
        self.particles.random.next_float()
    }

    fn rd(&mut self) -> f64 {
        self.particles.random.next_double()
    }

    fn ni(&mut self, bound: i32) -> i32 {
        self.particles.random.next_int(bound)
    }

    /// `ParticleUtils.spawnParticleBelow`.
    fn below(&mut self, pos: BlockPos, options: impl Into<Options>) {
        let at = DVec3::new(
            f64::from(pos.0) + self.rd(),
            f64::from(pos.1) - 0.05,
            f64::from(pos.2) + self.rd(),
        );
        self.spawn(options, at, DVec3::ZERO);
    }

    fn redstone(&mut self, at: DVec3, scale: f32) {
        self.spawn(Options::Dust([1.0, 0.0, 0.0], scale), at, DVec3::ZERO);
    }
}

impl Ambient {
    /// `ClientLevel.animateTick` for a player at `feet`, then the block
    /// entity tickers.
    pub fn tick(
        &mut self,
        world: &World,
        particles: &mut Particles,
        feet: BlockPos,
        sounds: &mut Vec<Sound>,
    ) {
        let mut emit = Emit {
            level: Level { world },
            particles,
            sounds,
            day_time: world.day.ticks.rem_euclid(24000.0),
        };
        for _ in 0..667 {
            for radius in [16, 32] {
                let axis = |emit: &mut Emit| emit.ni(radius) - emit.ni(radius);
                let pos = (
                    feet.0 + axis(&mut emit),
                    feet.1 + axis(&mut emit),
                    feet.2 + axis(&mut emit),
                );
                self.animate(&mut emit, pos);
            }
        }
        // The campfires' and spawners' own ticks, while their block stays.
        self.campfires.retain(|&pos| {
            emit.level.path(pos).ends_with("campfire")
                && emit.level.prop(pos, "lit") == Some("true")
                && distance2(pos, feet) < 64 * 64
        });
        for pos in self.campfires.clone() {
            campfire_tick(&mut emit, pos);
        }
        self.spawners
            .retain(|&pos| emit.level.path(pos) == "spawner" && distance2(pos, feet) < 64 * 64);
        for pos in self.spawners.clone() {
            // `BaseSpawner.clientTick`: with a player in range.
            if distance2(pos, feet) <= 16 * 16 {
                let at = corner(pos) + DVec3::new(emit.rd(), emit.rd(), emit.rd());
                emit.spawn(Type::Smoke, at, DVec3::ZERO);
                emit.spawn(Type::Flame, at, DVec3::ZERO);
            }
        }
    }

    /// `doAnimateTick` at one position.
    fn animate(&mut self, emit: &mut Emit, pos: BlockPos) {
        let Some(block) = emit.level.block(pos).cloned() else {
            return;
        };
        block_tick(emit, pos, &block);
        match block.id.path.as_str() {
            "campfire" | "soul_campfire" => {
                self.campfires.insert(pos);
            }
            "spawner" => {
                self.spawners.insert(pos);
            }
            _ => {}
        }
        if let Some(fluid) = emit.level.fluid(pos) {
            fluid_tick(emit, pos, fluid);
            if emit.ni(10) == 0 {
                let drip = if fluid.lava {
                    Type::DrippingLava
                } else {
                    Type::DrippingWater
                };
                let sturdy = emit.level.face_sturdy(pos, false);
                drips(emit, (pos.0, pos.1 - 1, pos.2), drip, sturdy);
            }
        }
    }
}

fn distance2(a: BlockPos, b: BlockPos) -> i32 {
    let (dx, dy, dz) = (a.0 - b.0, a.1 - b.1, a.2 - b.2);
    dx * dx + dy * dy + dz * dz
}

/// `ClientLevel.trySpawnDripParticles` under a fluid at `below`'s top.
fn drips(emit: &mut Emit, below: BlockPos, drip: Type, top_solid: bool) {
    if emit.level.fluid(below).is_some() {
        return;
    }
    let boxes = emit.level.boxes(below);
    let max_y = boxes.iter().map(|b| b[4]).fold(0.0, f64::max);
    let (bx, by, bz) = (f64::from(below.0), f64::from(below.1), f64::from(below.2));
    if max_y < 1.0 {
        if top_solid {
            let x = bx + emit.rd();
            let z = bz + emit.rd();
            emit.spawn(drip, DVec3::new(x, by + 1.0 - 0.05, z), DVec3::ZERO);
        }
        return;
    }
    let path = emit.level.path(below);
    if path.contains("glass") || path == "barrier" {
        return;
    }
    let min = |i: usize| boxes.iter().map(|b| b[i]).fold(1.0, f64::min);
    let max = |i: usize| boxes.iter().map(|b| b[i + 3]).fold(0.0, f64::max);
    let min_y = min(1);
    let y = if min_y > 0.0 {
        by + min_y - 0.05
    } else {
        let under = (below.0, below.1 - 1, below.2);
        let under_max = emit
            .level
            .boxes(under)
            .iter()
            .map(|b| b[4])
            .fold(0.0, f64::max);
        if under_max < 1.0 && emit.level.fluid(under).is_none() {
            by - 0.05
        } else {
            return;
        }
    };
    let x = bx + min(0) + emit.rd() * (max(0) - min(0));
    let z = bz + min(2) + emit.rd() * (max(2) - min(2));
    emit.spawn(drip, DVec3::new(x, y, z), DVec3::ZERO);
}

/// `WaterFluid.animateTick` and `LavaFluid.animateTick`.
fn fluid_tick(emit: &mut Emit, pos: BlockPos, fluid: Fluid) {
    let base = corner(pos);
    if !fluid.lava {
        if !fluid.source && !fluid.falling {
            if emit.ni(64) == 0 {
                let volume = emit.rf() * 0.25 + 0.75;
                let pitch = emit.rf() + 0.5;
                emit.sound("minecraft:block.water.ambient", center(pos), volume, pitch);
            }
        } else if emit.ni(10) == 0 {
            let at = base + DVec3::new(emit.rd(), emit.rd(), emit.rd());
            emit.spawn(Type::Underwater, at, DVec3::ZERO);
        }
        return;
    }
    if !emit.level.is_air((pos.0, pos.1 + 1, pos.2)) {
        return;
    }
    if emit.ni(100) == 0 {
        let at = base + DVec3::new(emit.rd(), 1.0, emit.rd());
        emit.spawn(Type::Lava, at, DVec3::ZERO);
        let volume = 0.2 + emit.rf() * 0.2;
        let pitch = 0.9 + emit.rf() * 0.15;
        emit.sound("minecraft:block.lava.pop", at, volume, pitch);
    }
    if emit.ni(200) == 0 {
        let volume = 0.2 + emit.rf() * 0.2;
        let pitch = 0.9 + emit.rf() * 0.15;
        emit.sound("minecraft:block.lava.ambient", base, volume, pitch);
    }
}

/// One block's `animateTick`.
fn block_tick(emit: &mut Emit, pos: BlockPos, block: &Block) {
    let path = block.id.path.as_str();
    let base = corner(pos);
    let prop = |key: &str| block.properties.get(key).map(String::as_str);
    let lit = prop("lit") == Some("true");
    match path {
        "torch" | "soul_torch" | "copper_torch" => {
            let at = base + DVec3::new(0.5, 0.7, 0.5);
            emit.spawn(Type::Smoke, at, DVec3::ZERO);
            emit.spawn(torch_flame(path), at, DVec3::ZERO);
        }
        "wall_torch" | "soul_wall_torch" | "copper_wall_torch" => {
            let (ox, _, oz) = step(prop("facing").unwrap_or("north"));
            let at = base
                + DVec3::new(
                    0.5 - 0.27 * f64::from(ox),
                    0.7 + 0.22,
                    0.5 - 0.27 * f64::from(oz),
                );
            emit.spawn(Type::Smoke, at, DVec3::ZERO);
            emit.spawn(torch_flame(path), at, DVec3::ZERO);
        }
        "redstone_torch" if lit => {
            let jitter = |e: &mut Emit| (e.rd() - 0.5) * 0.2;
            let at = base + DVec3::new(0.5 + jitter(emit), 0.7 + jitter(emit), 0.5 + jitter(emit));
            emit.redstone(at, 1.0);
        }
        "redstone_wall_torch" if lit => {
            let (ox, _, oz) = step(prop("facing").unwrap_or("north"));
            let jitter = |e: &mut Emit| (e.rd() - 0.5) * 0.2;
            let at = base
                + DVec3::new(
                    0.5 + jitter(emit) - 0.27 * f64::from(ox),
                    0.7 + jitter(emit) + 0.22,
                    0.5 + jitter(emit) - 0.27 * f64::from(oz),
                );
            emit.redstone(at, 1.0);
        }
        "redstone_ore" | "deepslate_redstone_ore" if lit => redstone_ore(emit, pos),
        "redstone_wire" => redstone_wire(emit, pos, block),
        "repeater" if prop("powered") == Some("true") => {
            let jitter = |e: &mut Emit| (e.rd() - 0.5) * 0.2;
            let (bx, by, bz) = (0.5 + jitter(emit), 0.4 + jitter(emit), 0.5 + jitter(emit));
            let delay: f64 = prop("delay").and_then(|d| d.parse().ok()).unwrap_or(1.0);
            let mut off = -5.0;
            if emit.rng().next_bool() {
                off = delay * 2.0 - 1.0;
            }
            off /= 16.0;
            let (fx, _, fz) = step(prop("facing").unwrap_or("north"));
            emit.redstone(
                base + DVec3::new(bx + off * f64::from(fx), by, bz + off * f64::from(fz)),
                1.0,
            );
        }
        "lever" if prop("powered") == Some("true") => {
            if emit.rf() < 0.25 {
                lever_particle(emit, pos, block, 0.5);
            }
        }
        "furnace" | "blast_furnace" if lit => {
            let at = base + DVec3::new(0.5, 0.0, 0.5);
            if emit.rd() < 0.1 {
                let event = if path == "furnace" {
                    "minecraft:block.furnace.fire_crackle"
                } else {
                    "minecraft:block.blastfurnace.fire_crackle"
                };
                emit.sound(event, at, 1.0, 1.0);
            }
            let facing = prop("facing").unwrap_or("north");
            let (fx, _, fz) = step(facing);
            let side = emit.rd() * 0.6 - 0.3;
            let dx = if fx != 0 { f64::from(fx) * 0.52 } else { side };
            let height = if path == "furnace" { 6.0 } else { 9.0 };
            let dy = emit.rd() * height / 16.0;
            let dz = if fz != 0 { f64::from(fz) * 0.52 } else { side };
            let at = at + DVec3::new(dx, dy, dz);
            emit.spawn(Type::Smoke, at, DVec3::ZERO);
            if path == "furnace" {
                emit.spawn(Type::Flame, at, DVec3::ZERO);
            }
        }
        "smoker" if lit => {
            if emit.rd() < 0.1 {
                emit.sound(
                    "minecraft:block.smoker.smoke",
                    base + DVec3::new(0.5, 0.0, 0.5),
                    1.0,
                    1.0,
                );
            }
            emit.spawn(Type::Smoke, base + DVec3::new(0.5, 1.1, 0.5), DVec3::ZERO);
        }
        "campfire" | "soul_campfire" if lit => {
            if emit.ni(10) == 0 {
                let volume = 0.5 + emit.rf();
                let pitch = emit.rf() * 0.7 + 0.6;
                emit.sound(
                    "minecraft:block.campfire.crackle",
                    center(pos),
                    volume,
                    pitch,
                );
            }
            if path == "campfire" && emit.ni(5) == 0 {
                let motion = DVec3::new(
                    f64::from(emit.rf() / 2.0),
                    5.0e-5,
                    f64::from(emit.rf() / 2.0),
                );
                emit.spawn(Type::Lava, base + DVec3::splat(0.5), motion);
            }
        }
        "fire" | "soul_fire" => fire(emit, pos),
        _ if path.ends_with("candle") || path.ends_with("candle_cake") => {
            if lit {
                candle(emit, pos, block);
            }
        }
        _ if path.ends_with("leaves") => leaves(emit, pos, block),
        "sand" | "red_sand" | "gravel" | "suspicious_sand" | "suspicious_gravel" | "anvil"
        | "chipped_anvil" | "damaged_anvil" | "dragon_egg" => falling_dust(emit, pos, block),
        _ if path.ends_with("concrete_powder") => falling_dust(emit, pos, block),
        "pointed_dripstone" => dripstone(emit, pos, block),
        "dead_bush" => {
            if emit.ni(130) == 0 {
                let below = emit.level.path((pos.0, pos.1 - 1, pos.2)).to_owned();
                if (below == "red_sand" || below.ends_with("terracotta")) && emit.ni(3) != 0 {
                    return;
                }
                if dry_ground(&emit.level, pos) {
                    emit.sound("minecraft:block.dead_bush.idle", base, 1.0, 1.0);
                }
            }
        }
        "short_dry_grass" | "tall_dry_grass" => {
            if emit.ni(200) == 0 && dry_ground(&emit.level, pos) {
                // `playPlayerSound`: at the listener.
                emit.sounds
                    .push(("minecraft:block.dry_grass.ambient", DVec3::NAN, 1.0, 1.0));
            }
        }
        "spore_blossom" => {
            let at = base + DVec3::new(emit.rd(), 0.7, emit.rd());
            emit.spawn(Type::FallingSporeBlossom, at, DVec3::ZERO);
            for _ in 0..14 {
                let x = pos.0 + emit.ni(21) - 10;
                let y = pos.1 - emit.ni(10);
                let z = pos.2 + emit.ni(21) - 10;
                if !emit.level.full_block((x, y, z)) {
                    let at = corner((x, y, z)) + DVec3::new(emit.rd(), emit.rd(), emit.rd());
                    emit.spawn(Type::SporeBlossomAir, at, DVec3::ZERO);
                }
            }
        }
        "end_rod" => {
            let b = base
                + DVec3::new(
                    0.55 - f64::from(emit.rf()) * 0.1,
                    0.55 - f64::from(emit.rf()) * 0.1,
                    0.55 - f64::from(emit.rf()) * 0.1,
                );
            let r = f64::from(0.4 - (emit.rf() + emit.rf()) * 0.4);
            if emit.ni(5) == 0 {
                let (sx, sy, sz) = step(prop("facing").unwrap_or("up"));
                let at = b + DVec3::new(f64::from(sx), f64::from(sy), f64::from(sz)) * r;
                let wobble = |e: &mut Emit| e.rng().next_gaussian() * 0.005;
                let motion = DVec3::new(wobble(emit), wobble(emit), wobble(emit));
                emit.spawn(Type::EndRod, at, motion);
            }
        }
        "enchanting_table" => enchanting_table(emit, pos),
        "brewing_stand" => {
            let at = base
                + DVec3::new(
                    0.4 + f64::from(emit.rf()) * 0.2,
                    0.7 + f64::from(emit.rf()) * 0.3,
                    0.4 + f64::from(emit.rf()) * 0.2,
                );
            emit.spawn(Type::Smoke, at, DVec3::ZERO);
        }
        "mycelium" => {
            if emit.ni(10) == 0 {
                let at = base + DVec3::new(emit.rd(), 1.1, emit.rd());
                emit.spawn(Type::Mycelium, at, DVec3::ZERO);
            }
        }
        "firefly_bush" => firefly_bush(emit, pos),
        "beehive" | "bee_nest" => {
            let honey: i32 = prop("honey_level")
                .and_then(|h| h.parse().ok())
                .unwrap_or(0);
            if honey >= 5 && emit.level.fluid(pos).is_none() && emit.rf() >= 0.3 {
                let under = (pos.0, pos.1 - 1, pos.2);
                if !emit.level.full_block(under) && emit.level.fluid(under).is_none() {
                    let at = base + DVec3::new(emit.rd(), -0.05, emit.rd());
                    emit.spawn(Type::DrippingHoney, at, DVec3::ZERO);
                }
            }
        }
        "wet_sponge" => wet_sponge(emit, pos),
        "wither_rose" => {
            for _ in 0..3 {
                if emit.rng().next_bool() {
                    let at = base
                        + DVec3::new(
                            0.5 + emit.rd() / 5.0,
                            0.5 - emit.rd(),
                            0.5 + emit.rd() / 5.0,
                        );
                    emit.spawn(Type::Smoke, at, DVec3::ZERO);
                }
            }
        }
        "ender_chest" => {
            for _ in 0..3 {
                let fx = f64::from(emit.ni(2) * 2 - 1);
                let fz = f64::from(emit.ni(2) * 2 - 1);
                let at = base + DVec3::new(0.5 + 0.25 * fx, f64::from(emit.rf()), 0.5 + 0.25 * fz);
                let motion = DVec3::new(
                    f64::from(emit.rf()) * fx,
                    f64::from(emit.rf() - 0.5) * 0.125,
                    f64::from(emit.rf()) * fz,
                );
                emit.spawn(Type::Portal, at, motion);
            }
        }
        "creaking_heart" => {
            let night = (12600.0..23401.0).contains(&emit.day_time);
            if night
                && prop("creaking_heart_state") != Some("uprooted")
                && emit.ni(16) == 0
                && ALL.iter().all(|&d| {
                    emit.level.path(offset(pos, d)).starts_with("pale_oak_")
                        && emit.level.path(offset(pos, d)).ends_with("log")
                })
            {
                emit.sound("minecraft:block.creaking_heart.idle", base, 1.0, 1.0);
            }
        }
        "open_eyeblossom" => {
            if emit.ni(700) == 0 && emit.level.path((pos.0, pos.1 - 1, pos.2)) == "pale_moss_block"
            {
                emit.sound("minecraft:block.eyeblossom.idle", base, 1.0, 1.0);
            }
        }
        "pale_hanging_moss" => {
            let above = emit.level.path((pos.0, pos.1 + 1, pos.2)).to_owned();
            if emit.ni(500) == 0
                && (above == "pale_oak_leaves"
                    || above.starts_with("pale_oak_") && above.ends_with("log"))
            {
                emit.sound("minecraft:block.pale_hanging_moss.idle", base, 1.0, 1.0);
            }
        }
        _ => {}
    }
    if matches!(path, "sand" | "red_sand") {
        sand_ambient(emit, pos);
    }
}

fn torch_flame(path: &str) -> Type {
    if path.starts_with("soul") {
        Type::SoulFireFlame
    } else if path.starts_with("copper") {
        Type::CopperFireFlame
    } else {
        Type::Flame
    }
}

/// `RedStoneOreBlock.spawnParticles`: a speck on each face open to air.
fn redstone_ore(emit: &mut Emit, pos: BlockPos) {
    for d in ALL {
        if emit.level.solid_render(offset(pos, d)) {
            continue;
        }
        let coord = |e: &mut Emit, s: i32| {
            if s != 0 {
                0.5 + 0.5625 * f64::from(s)
            } else {
                f64::from(e.rf())
            }
        };
        let at = corner(pos) + DVec3::new(coord(emit, d.0), coord(emit, d.1), coord(emit, d.2));
        emit.redstone(at, 1.0);
    }
}

/// `RedstoneWireBlock.animateTick`: specks along its powered lines.
fn redstone_wire(emit: &mut Emit, pos: BlockPos, block: &Block) {
    let power: u8 = block
        .properties
        .get("power")
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    if power == 0 {
        return;
    }
    let argb = minecraft_terrain::mesh::redstone_wire_color_argb(power);
    let colour = [
        ((argb >> 16) & 255) as f32 / 255.0,
        ((argb >> 8) & 255) as f32 / 255.0,
        (argb & 255) as f32 / 255.0,
    ];
    let line =
        |emit: &mut Emit, side: (i32, i32, i32), along: (i32, i32, i32), from: f32, to: f32| {
            let span = to - from;
            if emit.rf() >= 0.2 * span {
                return;
            }
            let t = f64::from(from + span * emit.rf());
            let axis = |s: i32, a: i32| 0.5 + 0.4375 * f64::from(s) + t * f64::from(a);
            let at = corner(pos)
                + DVec3::new(
                    axis(side.0, along.0),
                    axis(side.1, along.1),
                    axis(side.2, along.2),
                );
            emit.spawn(Options::Dust(colour, 1.0), at, DVec3::ZERO);
        };
    for (name, d) in [
        ("north", (0, 0, -1)),
        ("east", (1, 0, 0)),
        ("south", (0, 0, 1)),
        ("west", (-1, 0, 0)),
    ] {
        match block.properties.get(name).map(String::as_str) {
            Some("up") => {
                line(emit, d, (0, 1, 0), -0.5, 0.5);
                line(emit, (0, -1, 0), d, 0.0, 0.5);
            }
            Some("side") => line(emit, (0, -1, 0), d, 0.0, 0.5),
            _ => line(emit, (0, -1, 0), d, 0.0, 0.3),
        }
    }
}

/// `LeverBlock.makeParticle`.
pub fn lever_particle_at(
    particles: &mut Particles,
    world: &World,
    pos: BlockPos,
    block: &Block,
    scale: f32,
) {
    let mut sounds = Vec::new();
    let mut emit = Emit {
        level: Level { world },
        particles,
        sounds: &mut sounds,
        day_time: 0.0,
    };
    lever_particle(&mut emit, pos, block, scale);
}

fn lever_particle(emit: &mut Emit, pos: BlockPos, block: &Block, scale: f32) {
    let facing = block
        .properties
        .get("facing")
        .map_or("north", String::as_str);
    let (fx, fy, fz) = step(facing);
    let o = (-fx, -fy, -fz);
    let connected = match block.properties.get("face").map(String::as_str) {
        Some("ceiling") => (0, -1, 0),
        Some("floor") => (0, 1, 0),
        _ => (fx, fy, fz),
    };
    let c = (-connected.0, -connected.1, -connected.2);
    let axis = |a: i32, b: i32| 0.5 + 0.1 * f64::from(a) + 0.2 * f64::from(b);
    let at = corner(pos) + DVec3::new(axis(o.0, c.0), axis(o.1, c.1), axis(o.2, c.2));
    emit.redstone(at, scale);
}

/// `BaseFireBlock.animateTick`.
fn fire(emit: &mut Emit, pos: BlockPos) {
    let base = corner(pos);
    if emit.ni(24) == 0 {
        let volume = 1.0 + emit.rf();
        let pitch = emit.rf() * 0.7 + 0.3;
        emit.sound("minecraft:block.fire.ambient", center(pos), volume, pitch);
    }
    let below = (pos.0, pos.1 - 1, pos.2);
    if !burnable(emit.level.path(below)) && !emit.level.face_sturdy(below, true) {
        let sides: [((i32, i32, i32), fn(&mut Emit) -> DVec3); 5] = [
            ((-1, 0, 0), |e| DVec3::new(e.rd() * 0.1, e.rd(), e.rd())),
            ((1, 0, 0), |e| {
                DVec3::new(1.0 - e.rd() * 0.1, e.rd(), e.rd())
            }),
            ((0, 0, -1), |e| DVec3::new(e.rd(), e.rd(), e.rd() * 0.1)),
            ((0, 0, 1), |e| {
                DVec3::new(e.rd(), e.rd(), 1.0 - e.rd() * 0.1)
            }),
            ((0, 1, 0), |e| {
                DVec3::new(e.rd(), 1.0 - e.rd() * 0.1, e.rd())
            }),
        ];
        for (d, place) in sides {
            if burnable(emit.level.path(offset(pos, d))) {
                for _ in 0..2 {
                    let at = base + place(emit);
                    emit.spawn(Type::LargeSmoke, at, DVec3::ZERO);
                }
            }
        }
    } else {
        for _ in 0..3 {
            let at = base + DVec3::new(emit.rd(), emit.rd() * 0.5 + 0.5, emit.rd());
            emit.spawn(Type::LargeSmoke, at, DVec3::ZERO);
        }
    }
}

/// Blocks fire spreads to (`FireBlock.igniteOdds > 0`).
fn burnable(path: &str) -> bool {
    path.ends_with("_planks")
        || path.ends_with("_log")
        || path.ends_with("_wood")
        || path.ends_with("_leaves")
        || path.ends_with("_wool")
        || path.ends_with("_carpet")
        || path.ends_with("_fence")
        || path.ends_with("_fence_gate")
        || path.ends_with("_stairs") && !path.contains("stone") && !path.contains("brick")
        || path.ends_with("_slab") && !path.contains("stone") && !path.contains("brick")
        || matches!(
            path,
            "bookshelf"
                | "tnt"
                | "short_grass"
                | "tall_grass"
                | "fern"
                | "large_fern"
                | "vine"
                | "hay_block"
                | "dead_bush"
                | "dried_kelp_block"
                | "scaffolding"
                | "target"
                | "coal_block"
                | "bamboo"
                | "lectern"
                | "composter"
                | "beehive"
                | "bee_nest"
                | "azalea"
                | "flowering_azalea"
        )
}

/// `AbstractCandleBlock.animateTick`.
fn candle(emit: &mut Emit, pos: BlockPos, block: &Block) {
    let wicks: &[(f64, f64, f64)] = if block.id.path.ends_with("candle_cake") {
        &[(8.0, 16.0, 8.0)]
    } else {
        match block.properties.get("candles").map(String::as_str) {
            Some("2") => &[(6.0, 7.0, 8.0), (10.0, 8.0, 7.0)],
            Some("3") => &[(8.0, 5.0, 10.0), (6.0, 7.0, 8.0), (9.0, 8.0, 7.0)],
            Some("4") => &[
                (7.0, 5.0, 9.0),
                (10.0, 7.0, 9.0),
                (6.0, 7.0, 6.0),
                (9.0, 8.0, 6.0),
            ],
            _ => &[(8.0, 8.0, 8.0)],
        }
    };
    for &(x, y, z) in wicks {
        let at = corner(pos) + DVec3::new(x, y, z) / 16.0;
        let c = emit.rf();
        if c < 0.3 {
            emit.spawn(Type::Smoke, at, DVec3::ZERO);
            if c < 0.17 {
                let volume = 1.0 + emit.rf();
                let pitch = emit.rf() * 0.7 + 0.3;
                emit.sound(
                    "minecraft:block.candle.ambient",
                    at + DVec3::splat(0.5),
                    volume,
                    pitch,
                );
            }
        }
        emit.spawn(Type::SmallFlame, at, DVec3::ZERO);
    }
}

/// `LeavesBlock` and its falling-leaf subclasses.
fn leaves(emit: &mut Emit, pos: BlockPos, block: &Block) {
    let path = block.id.path.as_str();
    let (chance, particle) = match path {
        "cherry_leaves" => (0.1, Some(Options::Simple(Type::CherryLeaves))),
        "pale_oak_leaves" => (0.02, Some(Options::Simple(Type::PaleOakLeaves))),
        "spruce_leaves" => (0.0, None),
        "birch_leaves" => (0.01, Some(tinted(0x80a755))),
        "azalea_leaves" | "flowering_azalea_leaves" => (0.01, Some(tinted(0x70922d))),
        "oak_leaves" | "jungle_leaves" | "acacia_leaves" | "dark_oak_leaves"
        | "mangrove_leaves" => {
            let tint = minecraft_terrain::block_particles::terrain_tint(
                &emit.particles.tint,
                &emit.level.world.scene,
                pos,
                block,
            );
            (
                0.01,
                Some(Options::Color(
                    Type::TintedLeaves,
                    [1.0, tint[0], tint[1], tint[2]],
                )),
            )
        }
        _ => (0.0, None),
    };
    let Some(particle) = particle else {
        return;
    };
    if emit.rf() < chance {
        // The block below's top face isn't a full one.
        let below = (pos.0, pos.1 - 1, pos.2);
        if !emit.level.face_sturdy(below, true) {
            emit.below(pos, particle);
        }
    }
}

fn tinted(hex: u32) -> Options {
    Options::Color(
        Type::TintedLeaves,
        [
            1.0,
            ((hex >> 16) & 255) as f32 / 255.0,
            ((hex >> 8) & 255) as f32 / 255.0,
            (hex & 255) as f32 / 255.0,
        ],
    )
}

/// `FallingBlock.animateTick`: dust below a block that could fall.
fn falling_dust(emit: &mut Emit, pos: BlockPos, block: &Block) {
    if emit.ni(16) != 0 {
        return;
    }
    let below = (pos.0, pos.1 - 1, pos.2);
    let path = emit.level.path(below).to_owned();
    let free = emit.level.is_air(below)
        || path == "fire"
        || path == "soul_fire"
        || emit.level.fluid(below).is_some()
        || matches!(
            path.as_str(),
            "short_grass" | "tall_grass" | "fern" | "large_fern" | "dead_bush" | "vine"
        ) && !emit.level.full_block(below);
    if free {
        emit.below(pos, Options::Block(Type::FallingDust, block.clone()));
    }
}

/// `PointedDripstoneBlock.animateTick`: a tip drips what is above its root.
fn dripstone(emit: &mut Emit, pos: BlockPos, block: &Block) {
    let props = &block.properties;
    if props.get("vertical_direction").map(String::as_str) != Some("down")
        || props.get("thickness").map(String::as_str) != Some("tip")
        || props.get("waterlogged").map(String::as_str) == Some("true")
    {
        return;
    }
    let v = emit.rf();
    if v > 0.12 {
        return;
    }
    let mut root = None;
    for i in 1..=10 {
        let at = (pos.0, pos.1 + i, pos.2);
        match emit.level.block(at) {
            Some(b) if b.id.path == "pointed_dripstone" => {
                if b.properties.get("vertical_direction").map(String::as_str) != Some("down") {
                    return;
                }
            }
            Some(_) => {
                root = Some(at);
                break;
            }
            None => return,
        }
    }
    let Some(root) = root else {
        return;
    };
    let above = (root.0, root.1 + 1, root.2);
    let fluid = if emit.level.path(above) == "mud" {
        Some(false)
    } else {
        emit.level.fluid(above).map(|f| f.lava)
    };
    if v >= 0.02 && fluid.is_none() {
        return;
    }
    let seed = position_seed(pos.0, 0, pos.2);
    let off = |bits: i64| ((((bits & 15) as f32) / 15.0 - 0.5) * 0.5).clamp(-0.125, 0.125);
    let at = corner(pos)
        + DVec3::new(
            0.5 + f64::from(off(seed)),
            0.25,
            0.5 + f64::from(off(seed >> 8)),
        );
    let drip = if fluid == Some(true) {
        Type::DrippingDripstoneLava
    } else {
        Type::DrippingDripstoneWater
    };
    emit.spawn(drip, at, DVec3::ZERO);
}

/// Sand, red sand or terracotta under both of the two blocks below.
fn dry_ground(level: &Level, pos: BlockPos) -> bool {
    (1..=2).all(|d| {
        let path = level.path((pos.0, pos.1 - d, pos.2));
        path == "sand" || path == "red_sand" || path.ends_with("terracotta")
    })
}

/// `SandBlock`'s desert sound: open sand on all four sides eight away.
fn sand_ambient(emit: &mut Emit, pos: BlockPos) {
    if !emit.level.is_air((pos.0, pos.1 + 1, pos.2)) || emit.ni(2100) != 0 {
        return;
    }
    let sandy = |level: &Level, x: i32, z: i32| {
        (pos.1 - 5..=pos.1 + 5).rev().any(|y| {
            let path = level.path((x, y, z));
            (path == "sand" || path == "red_sand") && level.is_air((x, y + 1, z))
        })
    };
    if HORIZONTAL
        .iter()
        .all(|d| sandy(&emit.level, pos.0 + d.0 * 8, pos.2 + d.2 * 8))
    {
        emit.sound("minecraft:block.sand.idle", corner(pos), 1.0, 1.0);
    }
}

/// `EnchantingTableBlock.animateTick`: glyphs drift to the table from each
/// bookshelf it counts (`isValidBookShelf`, by the block tags), one in
/// sixteen each tick.
fn enchanting_table(emit: &mut Emit, pos: BlockPos) {
    use minecraftoss_player::menu::enchanting::{bookshelf_offsets, is_valid_bookshelf};
    let at = |[x, y, z]: [i32; 3]| (pos.0 + x, pos.1 + y, pos.2 + z);
    for offset in bookshelf_offsets() {
        if emit.ni(16) != 0 {
            continue;
        }
        let level = &emit.level;
        let provider = |o| level.in_tag(at(o), "minecraft:enchantment_power_provider");
        let transmitter = |o| level.in_tag(at(o), "minecraft:enchantment_power_transmitter");
        if !is_valid_bookshelf(offset, provider, transmitter) {
            continue;
        }
        let [ox, oy, oz] = offset;
        let motion = DVec3::new(
            f64::from(ox) + f64::from(emit.rf()) - 0.5,
            f64::from(oy) - f64::from(emit.rf()) - 1.0,
            f64::from(oz) + f64::from(emit.rf()) - 0.5,
        );
        emit.spawn(
            Type::Enchant,
            corner(pos) + DVec3::new(0.5, 2.0, 0.5),
            motion,
        );
    }
}

/// `FireflyBushBlock.animateTick`.
fn firefly_bush(emit: &mut Emit, pos: BlockPos) {
    let night = (12600.0..23401.0).contains(&emit.day_time);
    if emit.ni(30) == 0 && night && emit.level.world.light.get((pos.0, pos.1 + 1, pos.2)) >= 15 {
        emit.sound("minecraft:block.firefly_bush.idle", center(pos), 1.0, 1.0);
    }
    let light = emit.level.world.light.get(pos);
    let darken = (15.0 - emit.level.world.sky_light_level()).clamp(0.0, 15.0) as u8;
    let brightness = light
        .saturating_sub(darken)
        .max(emit.level.world.light.get_block(pos));
    if brightness <= 13 && emit.rd() <= 0.7 {
        let at = corner(pos)
            + DVec3::new(
                emit.rd() * 10.0 - 5.0,
                emit.rd() * 5.0,
                emit.rd() * 10.0 - 5.0,
            );
        emit.spawn(Type::Firefly, at, DVec3::ZERO);
    }
}

/// `WetSpongeBlock.animateTick`.
fn wet_sponge(emit: &mut Emit, pos: BlockPos) {
    let d = ALL[emit.ni(6) as usize];
    if d == (0, 1, 0) {
        return;
    }
    let neighbour = offset(pos, d);
    // The neighbour's face towards the sponge.
    let sturdy = match d {
        (0, -1, 0) => emit.level.face_sturdy(neighbour, true),
        _ => emit.level.full_block(neighbour),
    };
    if sturdy {
        return;
    }
    let base = corner(pos);
    let at = if d == (0, -1, 0) {
        base + DVec3::new(emit.rd(), -0.05, emit.rd())
    } else {
        let y = emit.rd() * 0.8;
        match d {
            (1, 0, 0) => base + DVec3::new(1.1, y, emit.rd()),
            (-1, 0, 0) => base + DVec3::new(0.05, y, emit.rd()),
            (0, 0, 1) => base + DVec3::new(emit.rd(), y, 1.1),
            _ => base + DVec3::new(emit.rd(), y, 0.05),
        }
    };
    emit.spawn(Type::DrippingWater, at, DVec3::ZERO);
}

/// `CampfireBlockEntity.particleTick` and `CampfireBlock.makeParticles`.
fn campfire_tick(emit: &mut Emit, pos: BlockPos) {
    if emit.rf() < 0.11 {
        let mut i = 0;
        while i < emit.ni(2) + 2 {
            campfire_smoke(emit, pos, false);
            i += 1;
        }
    }
}

fn campfire_smoke(emit: &mut Emit, pos: BlockPos, smoking: bool) {
    let signal = emit.level.path((pos.0, pos.1 - 1, pos.2)) == "hay_block";
    let sign = |e: &mut Emit| if e.rng().next_bool() { 1.0 } else { -1.0 };
    let x = 0.5 + emit.rd() / 3.0 * sign(emit);
    let y = emit.rd() + emit.rd();
    let z = 0.5 + emit.rd() / 3.0 * sign(emit);
    let kind = if signal {
        Type::CampfireSignalSmoke
    } else {
        Type::CampfireCosySmoke
    };
    emit.spawn(
        kind,
        corner(pos) + DVec3::new(x, y, z),
        DVec3::new(0.0, 0.07, 0.0),
    );
    if smoking {
        let x = 0.5 + emit.rd() / 4.0 * sign(emit);
        let z = 0.5 + emit.rd() / 4.0 * sign(emit);
        emit.spawn(
            Type::Smoke,
            corner(pos) + DVec3::new(x, 0.4, z),
            DVec3::new(0.0, 0.005, 0.0),
        );
    }
}

/// Level event 1501: lava hissing as it meets water.
#[allow(dead_code)]
pub fn lava_fizz(particles: &mut Particles, world: &World, pos: BlockPos, sounds: &mut Vec<Sound>) {
    let pitch = 2.6 + (particles.random.next_float() - particles.random.next_float()) * 0.8;
    sounds.push(("minecraft:block.lava.extinguish", center(pos), 0.5, pitch));
    for _ in 0..8 {
        let at = corner(pos)
            + DVec3::new(
                particles.random.next_double(),
                1.2,
                particles.random.next_double(),
            );
        particles.spawn(world, &Type::LargeSmoke.into(), at, DVec3::ZERO);
    }
}

/// Level event 1505: bone meal's sparkles, by what took it.
pub fn bone_meal(particles: &mut Particles, world: &World, pos: BlockPos, sounds: &mut Vec<Sound>) {
    sounds.push(("minecraft:item.bone_meal.use", center(pos), 1.0, 1.0));
    let path = world
        .block(pos)
        .map_or("air", |b| b.id.path.as_str())
        .to_owned();
    let spreads = matches!(
        path.as_str(),
        "grass_block"
            | "moss_block"
            | "pale_moss_block"
            | "crimson_nylium"
            | "warped_nylium"
            | "netherrack"
    ) || path == "water";
    let (origin, count, width, height, floating) = if spreads {
        ((pos.0, pos.1 + 1, pos.2), 45, 3.0, 1.0, false)
    } else {
        let height = world
            .collision_boxes(pos)
            .iter()
            .map(|b| b[4])
            .fold(0.0, f64::max);
        let height = if height == 0.0 { 1.0 } else { height };
        (pos, 15, 0.5, height, true)
    };
    let random = &mut particles.random;
    let mut spots = Vec::new();
    for _ in 0..count {
        let g = |r: &mut Random| r.next_gaussian() * 0.02;
        let motion = DVec3::new(g(random), g(random), g(random));
        let at = DVec3::new(
            f64::from(origin.0) + (0.5 - width) + random.next_double() * width * 2.0,
            f64::from(origin.1) + random.next_double() * height,
            f64::from(origin.2) + (0.5 - width) + random.next_double() * width * 2.0,
        );
        spots.push((at, motion));
    }
    for (at, motion) in spots {
        let below = (
            at.x.floor() as i32,
            at.y.floor() as i32 - 1,
            at.z.floor() as i32,
        );
        if floating
            || world
                .block(below)
                .is_some_and(|b| !matches!(b.id.path.as_str(), "air" | "cave_air"))
        {
            particles.spawn(world, &Type::HappyVillager.into(), at, motion);
        }
    }
}
