//! Vanilla's particle engine (`ParticleEngine`, `Particle`,
//! `SingleQuadParticle`): particles tick once a client tick with their own
//! gravity, friction and block collisions, and draw as camera-facing quads
//! from the sprites the pack's `particles/*.json` list, lit by the world at
//! their position. Each kind's provider and behaviour is in `kinds`.
use std::collections::HashMap;

use glam::{DVec3, Vec3};
use minecraft_terrain::mesh::{Atlas, BiomeTint, SectionVertex};
use minecraft_terrain::pack::{PackStack, ResourceId};

use crate::world::World;

pub use kinds::{Kind, Options, Type};

mod kinds;

/// `ParticleGroup`: at most this many of one render type, and past
/// `RESERVOIR_START` new ones are refused more often the fuller it is.
const MAX_PARTICLES: usize = 16384;
const RESERVOIR_START: usize = 12288;

/// Packed full brightness, as levels: sky 15, block 15.
pub const FULL_BRIGHT: Light = Light {
    sky: 240,
    block: 240,
};

/// Light as vanilla packs it for a vertex: each channel 0..=240 (level * 16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Light {
    pub sky: u8,
    pub block: u8,
}

/// A sprite list from `particles/<name>.json`, as atlas regions.
#[derive(Clone, Debug, Default)]
pub struct SpriteSet(Vec<[f32; 4]>);

impl SpriteSet {
    /// `SpriteSet.get(age, lifetime)`: the sprite for this point of a life.
    pub fn by_age(&self, age: i32, lifetime: i32) -> [f32; 4] {
        if self.0.is_empty() {
            return [0.0; 4];
        }
        let max = lifetime.max(1);
        let index = (age.max(0) as i64 * (self.0.len() as i64 - 1) / i64::from(max)) as usize;
        self.0[index.min(self.0.len() - 1)]
    }

    /// `SpriteSet.get(random)`.
    pub fn pick(&self, random: &mut Random) -> [f32; 4] {
        if self.0.is_empty() {
            return [0.0; 4];
        }
        self.0[random.next_int(self.0.len() as i32) as usize]
    }

    pub fn first(&self) -> [f32; 4] {
        self.0.first().copied().unwrap_or([0.0; 4])
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn get(&self, index: usize) -> [f32; 4] {
        self.0.get(index).copied().unwrap_or_else(|| self.first())
    }
}

/// Every particle's sprite set, by particle name (`flame`, `smoke`).
pub struct Sprites {
    sets: HashMap<String, SpriteSet>,
    empty: SpriteSet,
}

impl Sprites {
    /// The sprite lists the pack defines, resolved in the world atlas.
    pub fn load(packs: &PackStack, atlas: &Atlas) -> Self {
        let mut sets = HashMap::new();
        let files = packs
            .list("minecraft", "particles")
            .unwrap_or_else(|_| Vec::new());
        for path in files {
            let Some(name) = path
                .rsplit('/')
                .next()
                .and_then(|file| file.strip_suffix(".json"))
            else {
                continue;
            };
            let Ok(id) = ResourceId::parse(&format!("minecraft:{name}")) else {
                continue;
            };
            let Some(json) = packs
                .json(&id, &format!("particles/{name}.json"))
                .ok()
                .flatten()
            else {
                continue;
            };
            let textures = json["textures"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|texture| texture.as_str())
                        .filter_map(|texture| {
                            let (namespace, path) =
                                texture.split_once(':').unwrap_or(("minecraft", texture));
                            ResourceId::parse(&format!("{namespace}:particle/{path}")).ok()
                        })
                        .map(|texture| atlas.region(&texture))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(Vec::new);
            sets.insert(name.to_owned(), SpriteSet(textures));
        }
        Self {
            sets,
            empty: SpriteSet::default(),
        }
    }

    pub fn get(&self, name: &str) -> &SpriteSet {
        self.sets.get(name).unwrap_or(&self.empty)
    }
}

/// Particles' own unseeded random (`RandomSource.create()`): xorshift with
/// vanilla's distributions.
pub struct Random(u64);

impl Random {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn next_float(&mut self) -> f32 {
        ((self.next() >> 40) as u32 as f32) / ((1u32 << 24) as f32)
    }

    pub fn next_double(&mut self) -> f64 {
        ((self.next() >> 11) as f64) / ((1u64 << 53) as f64)
    }

    pub fn next_int(&mut self, bound: i32) -> i32 {
        if bound <= 0 {
            return 0;
        }
        ((self.next() >> 33) % bound as u64) as i32
    }

    pub fn next_bool(&mut self) -> bool {
        self.next() & (1 << 40) != 0
    }

    /// `nextGaussian`, by Box-Muller.
    pub fn next_gaussian(&mut self) -> f64 {
        let u = self.next_double().max(1e-12);
        let v = self.next_double();
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }

    /// `Mth.nextFloat(random, min, max)`.
    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        if min >= max {
            min
        } else {
            self.next_float() * (max - min) + min
        }
    }

    /// `random.triangle(mode, deviation)`.
    pub fn triangle(&mut self, mode: f64, deviation: f64) -> f64 {
        mode + deviation * (self.next_double() - self.next_double())
    }
}

/// `SingleQuadParticle.Layer`: drawn cut out, or blended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Opaque,
    Translucent,
}

/// `SingleQuadParticle.FacingCameraMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    /// Facing the camera on every axis.
    Camera,
    /// Turned only with the camera's yaw.
    Vertical,
}

/// A particle: `Particle` and `SingleQuadParticle`'s state, with its kind's
/// own in `kind`.
#[derive(Clone, Debug)]
pub struct Particle {
    pub kind: Kind,
    pub previous: DVec3,
    pub pos: DVec3,
    pub velocity: DVec3,
    /// The bounding box, as its minimum corner and size.
    bb_min: DVec3,
    pub bb_width: f32,
    pub bb_height: f32,
    pub on_ground: bool,
    pub has_physics: bool,
    stopped_by_collision: bool,
    pub removed: bool,
    pub age: i32,
    pub lifetime: i32,
    pub gravity: f32,
    pub friction: f32,
    pub speed_up_when_y_blocked: bool,
    pub quad_size: f32,
    pub rgb: [f32; 3],
    pub alpha: f32,
    pub roll: f32,
    pub previous_roll: f32,
    /// The atlas region drawn.
    pub sprite: [f32; 4],
    pub layer: Layer,
    pub facing: Facing,
}

impl Particle {
    /// `Particle(level, x, y, z)` and `SingleQuadParticle`'s size.
    pub fn at(kind: Kind, pos: DVec3, sprite: [f32; 4], random: &mut Random) -> Self {
        let mut particle = Self {
            kind,
            previous: pos,
            pos,
            velocity: DVec3::ZERO,
            bb_min: pos,
            bb_width: 0.6,
            bb_height: 1.8,
            on_ground: false,
            has_physics: true,
            stopped_by_collision: false,
            removed: false,
            age: 0,
            lifetime: 0,
            gravity: 0.0,
            friction: 0.98,
            speed_up_when_y_blocked: false,
            quad_size: 0.0,
            rgb: [1.0; 3],
            alpha: 1.0,
            roll: 0.0,
            previous_roll: 0.0,
            sprite,
            layer: Layer::Opaque,
            facing: Facing::Camera,
        };
        particle.set_size(0.2, 0.2);
        particle.set_pos(pos);
        particle.lifetime = (4.0 / (random.next_float() * 0.9 + 0.1)) as i32;
        particle.quad_size = 0.1 * (random.next_float() * 0.5 + 0.5) * 2.0;
        particle
    }

    /// `Particle(level, x, y, z, xa, ya, za)`: the given motion, scattered
    /// and normalised to a small random speed with a little lift.
    pub fn moving(
        kind: Kind,
        pos: DVec3,
        motion: DVec3,
        sprite: [f32; 4],
        random: &mut Random,
    ) -> Self {
        let mut particle = Self::at(kind, pos, sprite, random);
        let mut spread = || f64::from((random.next_float() * 2.0 - 1.0) * 0.4);
        let mut v = DVec3::new(
            motion.x + spread(),
            motion.y + spread(),
            motion.z + spread(),
        );
        let speed = f64::from((random.next_float() + random.next_float() + 1.0) * 0.15);
        let length = v.length();
        if length > 0.0 {
            v = v / length * speed * f64::from(0.4f32);
        }
        v.y += f64::from(0.1f32);
        particle.velocity = v;
        particle
    }

    /// `setPower`.
    pub fn set_power(&mut self, power: f32) {
        let power = f64::from(power);
        self.velocity.x *= power;
        self.velocity.y = (self.velocity.y - f64::from(0.1f32)) * power + f64::from(0.1f32);
        self.velocity.z *= power;
    }

    /// `SingleQuadParticle.scale`.
    pub fn scale(&mut self, scale: f32) {
        self.quad_size *= scale;
        self.set_size(0.2 * scale, 0.2 * scale);
    }

    pub fn set_size(&mut self, width: f32, height: f32) {
        if width != self.bb_width || height != self.bb_height {
            let center_x = self.bb_min.x + f64::from(self.bb_width) / 2.0;
            let center_z = self.bb_min.z + f64::from(self.bb_width) / 2.0;
            self.bb_width = width;
            self.bb_height = height;
            self.bb_min.x = center_x - f64::from(width) / 2.0;
            self.bb_min.z = center_z - f64::from(width) / 2.0;
        }
    }

    pub fn set_pos(&mut self, pos: DVec3) {
        self.pos = pos;
        let half = f64::from(self.bb_width / 2.0);
        self.bb_min = DVec3::new(pos.x - half, pos.y, pos.z - half);
    }

    fn bb_max(&self) -> DVec3 {
        self.bb_min
            + DVec3::new(
                f64::from(self.bb_width),
                f64::from(self.bb_height),
                f64::from(self.bb_width),
            )
    }

    /// `Particle.tick`: age, gravity, the move, friction, ground drag.
    pub fn base_tick(&mut self, world: &World) {
        self.previous = self.pos;
        let age = self.age;
        self.age += 1;
        if age >= self.lifetime {
            self.removed = true;
            return;
        }
        self.velocity.y -= 0.04 * f64::from(self.gravity);
        self.move_by(world, self.velocity);
        if self.speed_up_when_y_blocked && self.pos.y == self.previous.y {
            self.velocity.x *= 1.1;
            self.velocity.z *= 1.1;
        }
        let friction = f64::from(self.friction);
        self.velocity *= friction;
        if self.on_ground {
            self.velocity.x *= f64::from(0.7f32);
            self.velocity.z *= f64::from(0.7f32);
        }
    }

    /// `Particle.move`: against the blocks' collision shapes, stopping for
    /// good once a fall is caught.
    pub fn move_by(&mut self, world: &World, motion: DVec3) {
        if self.stopped_by_collision {
            return;
        }
        let original = motion;
        let mut motion = motion;
        if self.has_physics && motion != DVec3::ZERO && motion.length_squared() < 100.0 * 100.0 {
            motion = collide(world, self.bb_min, self.bb_max(), motion);
        }
        if motion != DVec3::ZERO {
            self.bb_min += motion;
            self.pos = DVec3::new(
                self.bb_min.x + f64::from(self.bb_width) / 2.0,
                self.bb_min.y,
                self.bb_min.z + f64::from(self.bb_width) / 2.0,
            );
        }
        if original.y.abs() >= 1.0e-5 && motion.y.abs() < 1.0e-5 {
            self.stopped_by_collision = true;
        }
        self.on_ground = original.y != motion.y && original.y < 0.0;
        if original.x != motion.x {
            self.velocity.x = 0.0;
        }
        if original.z != motion.z {
            self.velocity.z = 0.0;
        }
    }

    /// `Particle.move` for kinds that pass through blocks (flames): the box
    /// moves as asked.
    pub fn move_freely(&mut self, motion: DVec3) {
        self.bb_min += motion;
        self.pos = DVec3::new(
            self.bb_min.x + f64::from(self.bb_width) / 2.0,
            self.bb_min.y,
            self.bb_min.z + f64::from(self.bb_width) / 2.0,
        );
    }

    /// `getLightCoords`: the world's light where the particle is.
    pub fn world_light(&self, world: &World) -> Light {
        let block = (
            self.pos.x.floor() as i32,
            self.pos.y.floor() as i32,
            self.pos.z.floor() as i32,
        );
        if !world.chunk_ready(block) {
            return Light { sky: 240, block: 0 };
        }
        Light {
            sky: world.light.get(block).min(15) * 16,
            block: world.light.get_block(block).min(15) * 16,
        }
    }

    pub fn set_sprite_from_age(&mut self, set: &SpriteSet) {
        if !self.removed {
            self.sprite = set.by_age(self.age, self.lifetime);
        }
    }
}

/// `Entity.collideBoundingBox` for a box against the blocks: the motion
/// each axis can make, Y first, then the smaller horizontal axis last.
fn collide(world: &World, min: DVec3, max: DVec3, motion: DVec3) -> DVec3 {
    let swept_min = min.min(min + motion);
    let swept_max = max.max(max + motion);
    let mut boxes = Vec::new();
    let low = (swept_min - DVec3::splat(1.0e-7)).floor();
    let high = (swept_max + DVec3::splat(1.0e-7)).floor();
    // Fences and walls reach half a block above their cell.
    for x in low.x as i32..=high.x as i32 {
        for y in low.y as i32 - 1..=high.y as i32 {
            for z in low.z as i32..=high.z as i32 {
                for b in world.collision_boxes((x, y, z)) {
                    let offset = DVec3::new(f64::from(x), f64::from(y), f64::from(z));
                    boxes.push((
                        offset + DVec3::new(b[0], b[1], b[2]),
                        offset + DVec3::new(b[3], b[4], b[5]),
                    ));
                }
            }
        }
    }
    if boxes.is_empty() {
        return motion;
    }
    let order: [usize; 3] = if motion.x.abs() < motion.z.abs() {
        [1, 2, 0]
    } else {
        [1, 0, 2]
    };
    let mut resolved = DVec3::ZERO;
    for axis in order {
        let distance = motion[axis];
        if distance == 0.0 {
            continue;
        }
        let moved_min = min + resolved;
        let moved_max = max + resolved;
        resolved[axis] = collide_axis(axis, moved_min, moved_max, &boxes, distance);
    }
    resolved
}

/// `Shapes.collide` along one axis for boxes.
fn collide_axis(
    axis: usize,
    min: DVec3,
    max: DVec3,
    boxes: &[(DVec3, DVec3)],
    mut distance: f64,
) -> f64 {
    const EPSILON: f64 = 1.0e-7;
    let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
    for (box_min, box_max) in boxes {
        if distance.abs() < EPSILON {
            return 0.0;
        }
        let overlaps = |i: usize| box_max[i] > min[i] + EPSILON && box_min[i] < max[i] - EPSILON;
        if !overlaps(b) || !overlaps(c) {
            continue;
        }
        if distance > 0.0 {
            if box_min[axis] >= max[axis] - EPSILON {
                let room = box_min[axis] - max[axis];
                if room >= -EPSILON {
                    distance = distance.min(room);
                }
            }
        } else if box_max[axis] <= min[axis] + EPSILON {
            let room = box_max[axis] - min[axis];
            if room <= EPSILON {
                distance = distance.max(room);
            }
        }
    }
    distance
}

/// The camera a frame's particles face.
pub struct Camera {
    pub eye: DVec3,
    /// The camera's right and up, unit vectors in the world.
    pub right: Vec3,
    pub up: Vec3,
    /// The camera's yaw in radians, for vertical particles, and its pitch.
    pub yaw: f32,
    pub pitch: f32,
}

/// One frame's quads: cut out, then blended.
#[derive(Default)]
pub struct ParticleMesh {
    pub opaque: (Vec<SectionVertex>, Vec<u32>),
    pub translucent: (Vec<SectionVertex>, Vec<u32>),
}

/// `ParticleEngine`.
pub struct Particles {
    pub sprites: Sprites,
    pub random: Random,
    /// Biome colours, for block particles' tints.
    pub tint: BiomeTint,
    /// The player's feet and vertical speed, for clouds settling on them.
    pub player: Option<(DVec3, f64)>,
    /// Sounds particles made (drips landing): event, where, volume, pitch.
    pub sounds: Vec<(&'static str, DVec3, f32, f32)>,
    live: Vec<Particle>,
    adding: Vec<Particle>,
}

impl Particles {
    pub fn new(packs: &PackStack, atlas: &Atlas, seed: u64) -> Self {
        Self {
            sprites: Sprites::load(packs, atlas),
            random: Random::new(seed ^ 0x2545_f491_4f6c_dd1d),
            tint: BiomeTint::from_pack(packs).unwrap_or_else(|_| BiomeTint::empty()),
            player: None,
            sounds: Vec::new(),
            live: Vec::new(),
            adding: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// `ParticleEngine.createParticle`: the kind's provider makes it, and it
    /// joins at the next tick.
    pub fn spawn(&mut self, world: &World, options: &Options, pos: DVec3, motion: DVec3) {
        if let Some(particle) = self.make(world, options, pos, motion) {
            self.add(particle);
        }
    }

    /// The particle a provider makes, to adjust before adding it.
    pub fn make(
        &mut self,
        world: &World,
        options: &Options,
        pos: DVec3,
        motion: DVec3,
    ) -> Option<Particle> {
        kinds::create(self, world, options, pos, motion)
    }

    /// `ParticleEngine.destroy` (level event 2001): a grid of fragments
    /// over each box of the block's shape, flying out from its middle.
    pub fn destroy(
        &mut self,
        world: &World,
        pos: (i32, i32, i32),
        block: &minecraft_terrain::scene::Block,
    ) {
        let options = Options::Block(Type::Block, block.clone());
        let base = DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
        for b in crate::target::shape(world, pos, block) {
            let size = DVec3::new(
                (b[3] - b[0]).min(1.0),
                (b[4] - b[1]).min(1.0),
                (b[5] - b[2]).min(1.0),
            );
            let count = |w: f64| ((w / 0.25).ceil() as i32).max(2);
            let (nx, ny, nz) = (count(size.x), count(size.y), count(size.z));
            for i in 0..nx {
                for j in 0..ny {
                    for k in 0..nz {
                        let rel = DVec3::new(
                            (f64::from(i) + 0.5) / f64::from(nx),
                            (f64::from(j) + 0.5) / f64::from(ny),
                            (f64::from(k) + 0.5) / f64::from(nz),
                        );
                        let at = base + rel * size + DVec3::new(b[0], b[1], b[2]);
                        if let Some(particle) =
                            self.make(world, &options, at, rel - DVec3::splat(0.5))
                        {
                            self.add(particle);
                        }
                    }
                }
            }
        }
    }

    /// `ParticleEngine.crack`: one fragment off the face being mined.
    pub fn crack(
        &mut self,
        world: &World,
        pos: (i32, i32, i32),
        face: (i32, i32, i32),
        block: &minecraft_terrain::scene::Block,
    ) {
        let boxes = crate::target::shape(world, pos, block);
        if boxes.is_empty() {
            return;
        }
        let min = |i: usize| boxes.iter().map(|b| b[i]).fold(f64::INFINITY, f64::min);
        let max = |i: usize| {
            boxes
                .iter()
                .map(|b| b[i + 3])
                .fold(f64::NEG_INFINITY, f64::max)
        };
        let base = DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
        let mut at = DVec3::ZERO;
        for axis in 0..3 {
            at[axis] = base[axis]
                + self.random.next_double() * (max(axis) - min(axis) - 0.2)
                + 0.1
                + min(axis);
        }
        let step = [face.0, face.1, face.2];
        for axis in 0..3 {
            match step[axis] {
                -1 => at[axis] = base[axis] + min(axis) - 0.1,
                1 => at[axis] = base[axis] + max(axis) + 0.1,
                _ => {}
            }
        }
        let options = Options::Block(Type::Block, block.clone());
        if let Some(mut particle) = self.make(world, &options, at, DVec3::ZERO) {
            particle.set_power(0.2);
            particle.scale(0.6);
            self.add(particle);
        }
    }

    pub fn add(&mut self, particle: Particle) {
        self.adding.push(particle);
    }

    /// `ParticleEngine.tick`: every particle, then the new ones join.
    pub fn tick(&mut self, world: &World) {
        let mut live = std::mem::take(&mut self.live);
        for particle in &mut live {
            kinds::tick(particle, self, world);
        }
        live.retain(|particle| !particle.removed);
        self.live = live;
        for particle in std::mem::take(&mut self.adding) {
            let count = self.live.len();
            if count >= MAX_PARTICLES {
                continue;
            }
            if count >= RESERVOIR_START {
                let free =
                    (MAX_PARTICLES - count) as f32 / (MAX_PARTICLES - RESERVOIR_START) as f32;
                if self.random.next_float() >= free * free {
                    continue;
                }
            }
            self.live.push(particle);
        }
    }

    pub fn clear(&mut self) {
        self.live.clear();
        self.adding.clear();
    }

    /// The quads to draw, each facing the camera at its interpolated place.
    pub fn mesh(&self, world: &World, camera: &Camera, partial: f32) -> ParticleMesh {
        let mut mesh = ParticleMesh::default();
        let forward = camera.up.cross(camera.right);
        for particle in &self.live {
            let pos = particle.previous.lerp(particle.pos, f64::from(partial));
            // `QuadParticleGroup`: only those in front of the camera.
            let rel = (pos - camera.eye).as_vec3();
            if rel.dot(forward) < -1.0 && rel.length_squared() > 4.0 {
                continue;
            }
            let size = kinds::quad_size(particle, partial);
            let light = kinds::light(particle, world, partial);
            let [u0, v0, u1, v1] = particle.sprite;
            let (mut right, mut up) = (camera.right, camera.up);
            let mut scale = size;
            if particle.facing == Facing::Vertical {
                // `LOOKAT_Y`: the camera's rotation with its pitch dropped,
                // which also shrinks the quad by cos^2 of half the pitch.
                let (sin, cos) = camera.yaw.sin_cos();
                right = Vec3::new(-cos, 0.0, -sin);
                up = Vec3::Y;
                scale *= (camera.pitch / 2.0).cos().powi(2);
            }
            let roll = particle.previous_roll + (particle.roll - particle.previous_roll) * partial;
            if roll != 0.0 {
                let (sin, cos) = roll.sin_cos();
                let (r, u) = (right, up);
                right = r * cos + u * sin;
                up = u * cos - r * sin;
            }
            let center = pos.as_vec3();
            let channel = |v: f32| (v * 255.0).floor().clamp(0.0, 255.0) as u8;
            let colour = [
                channel(particle.rgb[0]),
                channel(particle.rgb[1]),
                channel(particle.rgb[2]),
                channel(kinds::alpha(particle, partial)),
            ];
            let target = match particle.layer {
                Layer::Opaque => &mut mesh.opaque,
                Layer::Translucent => &mut mesh.translucent,
            };
            let base = target.0.len() as u32;
            for (x, y, u, v) in [
                (1.0, -1.0, u1, v1),
                (1.0, 1.0, u1, v0),
                (-1.0, 1.0, u0, v0),
                (-1.0, -1.0, u0, v1),
            ] {
                let corner = center + (right * x + up * y) * scale;
                target.0.push(SectionVertex {
                    position: corner.to_array(),
                    uv: [u, v],
                    color: colour,
                    light: [light.sky, light.block],
                    pad: [0; 2],
                });
            }
            target
                .1
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        mesh
    }
}
