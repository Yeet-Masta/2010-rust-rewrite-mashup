//! Particles the player and mobs give off: crits, sweeps, damage hearts,
//! love hearts, sprint dust, landing dust, splashes and crumbs of food
//! (`TrackingEmitter`, `Entity`, `LivingEntity` and the packets the server
//! sends for them).

use glam::DVec3;
use minecraft_terrain::scene::Block;

use crate::particles::{Options, Particles, Type};
use crate::world::World;

/// A mob's feet, width and height.
pub type Bounds = (DVec3, f32, f32);

/// `TrackingEmitter`: bursts of a particle around a mob for a few ticks.
struct Tracking {
    entity: u64,
    kind: Type,
    life: i32,
}

#[derive(Default)]
pub struct Emitters {
    tracking: Vec<Tracking>,
}

impl Emitters {
    /// `ParticleEngine.createTrackingEmitter`: the first burst at once,
    /// then one a tick until three have gone.
    pub fn track(
        &mut self,
        particles: &mut Particles,
        world: &World,
        entity: u64,
        bounds: Option<Bounds>,
        kind: Type,
    ) {
        let Some(bounds) = bounds else {
            return;
        };
        burst(particles, world, bounds, kind);
        self.tracking.push(Tracking {
            entity,
            kind,
            life: 1,
        });
    }

    pub fn tick(
        &mut self,
        particles: &mut Particles,
        world: &World,
        bounds: impl Fn(u64) -> Option<Bounds>,
    ) {
        self.tracking.retain_mut(|emitter| {
            // The emitter follows its mob; one that is gone stops it.
            let Some(at) = bounds(emitter.entity) else {
                return false;
            };
            burst(particles, world, at, emitter.kind);
            emitter.life += 1;
            emitter.life < 3
        });
    }
}

/// `TrackingEmitter.tick`: up to 16 particles from inside a ball around
/// the mob's middle, flying outward.
fn burst(particles: &mut Particles, world: &World, (feet, width, height): Bounds, kind: Type) {
    let (width, height) = (f64::from(width), f64::from(height));
    for _ in 0..16 {
        let random = &mut particles.random;
        let xa = f64::from(random.next_float() * 2.0 - 1.0);
        let ya = f64::from(random.next_float() * 2.0 - 1.0);
        let za = f64::from(random.next_float() * 2.0 - 1.0);
        if xa * xa + ya * ya + za * za > 1.0 {
            continue;
        }
        let at = DVec3::new(
            feet.x + width * (xa / 4.0),
            feet.y + height * (0.5 + ya / 4.0),
            feet.z + width * (za / 4.0),
        );
        particles.spawn(world, &kind.into(), at, DVec3::new(xa, ya + 0.2, za));
    }
}

/// `ClientPacketListener.handleParticleEvent`: one particle moving at
/// `speed` along `spread` when `count` is 0, else `count` scattered by
/// `spread` with random speeds.
pub fn send(
    particles: &mut Particles,
    world: &World,
    options: &Options,
    at: DVec3,
    count: i32,
    spread: DVec3,
    speed: f64,
) {
    if count == 0 {
        particles.spawn(world, options, at, spread * speed);
        return;
    }
    for _ in 0..count {
        let random = &mut particles.random;
        let offset = DVec3::new(
            random.next_gaussian() * spread.x,
            random.next_gaussian() * spread.y,
            random.next_gaussian() * spread.z,
        );
        let motion = DVec3::new(
            random.next_gaussian() * speed,
            random.next_gaussian() * speed,
            random.next_gaussian() * speed,
        );
        particles.spawn(world, options, at + offset, motion);
    }
}

/// `count` particles somewhere in a mob's box, lifted by `lift`, drifting
/// slowly: love hearts (`Animal.handleEntityEvent`, `lift` 0.5) and a
/// villager's moods (`AbstractVillager.addParticlesAroundSelf`, 1).
pub fn around(
    particles: &mut Particles,
    world: &World,
    (feet, width, height): Bounds,
    kind: Type,
    count: usize,
    lift: f64,
) {
    let (width, height) = (f64::from(width), f64::from(height));
    for _ in 0..count {
        let random = &mut particles.random;
        let motion = DVec3::new(
            random.next_gaussian() * 0.02,
            random.next_gaussian() * 0.02,
            random.next_gaussian() * 0.02,
        );
        // `getRandomX(1)`, `getRandomY()`, `getRandomZ(1)`.
        let at = DVec3::new(
            feet.x + width * (2.0 * random.next_double() - 1.0),
            feet.y + height * random.next_double() + lift,
            feet.z + width * (2.0 * random.next_double() - 1.0),
        );
        particles.spawn(world, &kind.into(), at, motion);
    }
}

/// What a client does with `ClientboundEntityEventPacket`'s particle
/// events.
pub fn entity_event(particles: &mut Particles, world: &World, bounds: Bounds, event: u8) {
    match event {
        // `TamableAnimal.spawnTamingParticles`: hearts, or smoke when the
        // taming failed; `Animal`'s love hearts.
        7 | 18 => around(particles, world, bounds, Type::Heart, 7, 0.5),
        6 => around(particles, world, bounds, Type::Smoke, 7, 0.5),
        // `Villager.handleEntityEvent`.
        12 => around(particles, world, bounds, Type::Heart, 5, 1.0),
        13 => around(particles, world, bounds, Type::AngryVillager, 5, 1.0),
        14 => around(particles, world, bounds, Type::HappyVillager, 5, 1.0),
        _ => {}
    }
}

/// Blocks with nothing drawn, which make no sprint dust
/// (`RenderShape.INVISIBLE`).
fn invisible(block: &Block) -> bool {
    matches!(
        block.id.path.as_str(),
        "air"
            | "cave_air"
            | "void_air"
            | "water"
            | "lava"
            | "bubble_column"
            | "light"
            | "barrier"
            | "structure_void"
            | "moving_piston"
            | "end_portal"
            | "end_gateway"
    )
}

/// `Entity.spawnSprintParticle`: a fleck of the block underfoot, kicked
/// back against the motion. `under` is the block 0.2 below the feet.
pub fn sprint(
    particles: &mut Particles,
    world: &World,
    feet: DVec3,
    velocity: DVec3,
    width: f64,
    under: ((i32, i32, i32), &Block),
) {
    let (pos, block) = under;
    if invisible(block) {
        return;
    }
    let random = &mut particles.random;
    let mut x = feet.x + (random.next_double() - 0.5) * width;
    let mut z = feet.z + (random.next_double() - 0.5) * width;
    // On a block's edge the fleck stays over the block beneath.
    if feet.x.floor() as i32 != pos.0 {
        x = x.clamp(f64::from(pos.0), f64::from(pos.0) + 1.0);
    }
    if feet.z.floor() as i32 != pos.2 {
        z = z.clamp(f64::from(pos.2), f64::from(pos.2) + 1.0);
    }
    particles.spawn(
        world,
        &Options::Block(Type::Block, block.clone()),
        DVec3::new(x, feet.y + 0.1, z),
        DVec3::new(velocity.x * -4.0, 1.5, velocity.z * -4.0),
    );
}

/// `LivingEntity.checkFallDamage`: a ring of the landed-on block's dust,
/// more the harder the fall. `fall` is the fall distance at landing.
pub fn landing(
    particles: &mut Particles,
    world: &World,
    feet: DVec3,
    fall: f32,
    under: ((i32, i32, i32), &Block),
) {
    let (pos, block) = under;
    // `calculateFallPower`: past the safe fall distance of 3.
    let power = ((f64::from(fall) + 1.0e-6 - 3.0).floor()).max(0.0);
    if power <= 0.0 || block.id.path.ends_with("air") {
        return;
    }
    let (mut x, mut z) = (feet.x, feet.z);
    if pos.0 != feet.x.floor() as i32 || pos.2 != feet.z.floor() as i32 {
        let dx = x - f64::from(pos.0) - 0.5;
        let dz = z - f64::from(pos.2) - 0.5;
        let most = dx.abs().max(dz.abs());
        x = f64::from(pos.0) + 0.5 + dx / most * 0.5;
        z = f64::from(pos.2) + 0.5 + dz / most * 0.5;
    }
    let scale = (f64::from(0.2f32) + power / 15.0).min(2.5);
    send(
        particles,
        world,
        &Options::Block(Type::Block, block.clone()),
        DVec3::new(x, feet.y, z),
        (150.0 * scale) as i32,
        DVec3::ZERO,
        f64::from(0.15f32),
    );
}

/// `Entity.doWaterSplashEffect`'s particles: bubbles and splashes over the
/// water around the feet, carried along with the motion.
pub fn splash(particles: &mut Particles, world: &World, feet: DVec3, velocity: DVec3, width: f64) {
    let y = feet.y.floor() + 1.0;
    let count = (1.0 + width * 20.0).ceil() as usize;
    for kind in [Type::Bubble, Type::Splash] {
        for _ in 0..count {
            let random = &mut particles.random;
            let xo = (random.next_double() * 2.0 - 1.0) * width;
            let zo = (random.next_double() * 2.0 - 1.0) * width;
            let mut motion = velocity;
            if kind == Type::Bubble {
                motion.y -= random.next_double() * f64::from(0.2f32);
            }
            particles.spawn(
                world,
                &kind.into(),
                DVec3::new(feet.x + xo, y, feet.z + zo),
                motion,
            );
        }
    }
}

/// `LivingEntity.spawnItemParticles`: crumbs of an item falling from in
/// front of the mouth (eating, and an item breaking).
pub fn item_crumbs(
    particles: &mut Particles,
    world: &World,
    item: &str,
    count: usize,
    feet: DVec3,
    eye_height: f64,
    yaw: f64,
    pitch: f64,
) {
    let options = Options::Item(item.to_owned());
    let (x_rot, y_rot) = (-pitch.to_radians() as f32, -yaw.to_radians() as f32);
    // `Vec3.xRot` then `Vec3.yRot`.
    let turn = |v: DVec3| {
        let (s, c) = (f64::from(x_rot.sin()), f64::from(x_rot.cos()));
        let v = DVec3::new(v.x, v.y * c + v.z * s, v.z * c - v.y * s);
        let (s, c) = (f64::from(y_rot.sin()), f64::from(y_rot.cos()));
        DVec3::new(v.x * c + v.z * s, v.y, v.z * c - v.x * s)
    };
    for _ in 0..count {
        let random = &mut particles.random;
        let d = DVec3::new(
            (f64::from(random.next_float()) - 0.5) * 0.1,
            f64::from(random.next_float()) * 0.1 + 0.1,
            0.0,
        );
        let d = turn(d);
        let y = -f64::from(random.next_float()) * 0.6 - 0.3;
        let p = DVec3::new((f64::from(random.next_float()) - 0.5) * 0.3, y, 0.6);
        let p = turn(p) + feet + DVec3::new(0.0, eye_height, 0.0);
        particles.spawn(world, &options, p, DVec3::new(d.x, d.y + 0.05, d.z));
    }
}
