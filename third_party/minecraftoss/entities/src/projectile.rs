//! Source-informed Java 26.3 arrow launch, flight, block and entity hits.
//! Provenance: Projectile.getMovementToShoot/shoot/shootFromRotation/
//! checkLeftOwner, AbstractArrow.tick/stepMoveAndHit/onHitBlock/
//! onHitEntity/shouldFall/startFalling/tickDespawn/playerTouch,
//! ProjectileUtil.getManyEntityHitResult, ProjectileDeflection.REVERSE,
//! AABB.clip, Level.clipIncludingBorder, and
//! AbstractSkeleton.performRangedAttack. The world applies each struck
//! entity's damage and reports back whether it took it.

use glam::DVec3;
use minecraftoss_player::inventory::ItemStack;
use minecraftoss_player::{collision_boxes_at, rng::LegacyRandom, Block, Pos, World};

/// `AbstractArrow.Pickup`: what a player touching a landed arrow takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pickup {
    #[default]
    Disallowed,
    Allowed,
    /// Only a player with infinite materials takes it (it gives nothing).
    CreativeOnly,
}

/// Which `AbstractArrow` it is (`EntityTypes.ARROW`, `SPECTRAL_ARROW`,
/// `TRIDENT`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArrowKind {
    #[default]
    Arrow,
    Spectral,
    Trident,
}

impl ArrowKind {
    /// Its entity type's ID.
    pub fn type_id(self) -> &'static str {
        match self {
            Self::Arrow => "minecraft:arrow",
            Self::Spectral => "minecraft:spectral_arrow",
            Self::Trident => "minecraft:trident",
        }
    }

    /// `getWaterInertia`: a trident keeps its speed under water.
    fn water_inertia(self) -> f32 {
        if self == Self::Trident { 0.99 } else { 0.6 }
    }

    /// `getDefaultHitGroundSoundEvent`.
    pub fn hit_ground_sound(self) -> &'static str {
        if self == Self::Trident { "item.trident.hit_ground" } else { "entity.arrow.hit" }
    }

    /// `getDefaultPickupItem`.
    pub fn default_item(self) -> &'static str {
        match self {
            Self::Arrow => "minecraft:arrow",
            Self::Spectral => "minecraft:spectral_arrow",
            Self::Trident => "minecraft:trident",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Arrow {
    pub position: DVec3,
    pub velocity: DVec3,
    pub in_ground: bool,
    pub in_ground_time: i32,
    pub life: i32,
    pub shake_time: i32,
    pub alive: bool,
    pub impact_block: Option<Pos>,
    pub base_damage: f64,
    pub kind: ArrowKind,
    /// `isCritArrow`: a fully drawn bow's, or any player's crossbow bolt.
    pub crit: bool,
    /// `getPierceLevel`.
    pub pierce_level: u8,
    pub pickup: Pickup,
    /// `pickupItemStack`: what a pickup gives; a tipped arrow's potion.
    pub item: Option<ItemStack>,
    /// `firedFromWeapon`: its enchantments act on the hits.
    pub weapon: Option<ItemStack>,
    /// `remainingFireTicks`: flame sets it alight; water puts it out.
    pub fire_ticks: i32,
    /// `wasTouchingWater` from its last flight tick.
    pub in_water: bool,
    /// `noPhysics`: a returning trident flies through everything.
    pub no_physics: bool,
    /// `leftOwner`: it has flown clear of its shooter.
    pub left_owner: bool,
    /// `getYRot` and `getXRot`, for drawing.
    pub yaw: f32,
    pub pitch: f32,
    /// `soundEvent`: what it plays as it strikes (a crossbow's bolt its
    /// own, then `entity.arrow.hit` once it has landed).
    pub sound: &'static str,
    /// A trident's `dealtDamage` (it struck, or has lain in the ground),
    /// its loyalty (`ID_LOYALTY`) and the ticks it has flown back
    /// (`clientSideReturnTridentTickCount`).
    pub dealt_damage: bool,
    pub loyalty: u8,
    pub return_ticks: i32,
    /// `piercingIgnoreEntityIds`.
    pierced: Vec<u64>,
    impact_state: Option<Block>,
    random: LegacyRandom,
    /// Where it played its hit sound this tick: event, position, pitch.
    hit_sound: Option<(&'static str, DVec3, f32)>,
}

#[derive(Clone, Copy, Debug)]
pub struct ArrowTarget {
    pub id: u64,
    pub min: DVec3,
    pub max: DVec3,
    /// Its feet (`position`), which orders the hits.
    pub feet: DVec3,
}

#[derive(Clone, Copy, Debug)]
pub struct ArrowImpact {
    pub target_id: u64,
    /// `ceil(speed * baseDamage)` before the weapon's enchantments and a
    /// critical arrow's bonus.
    pub damage: f32,
    pub velocity: DVec3,
}

/// This tick's flight (`stepMoveAndHit`): the block it runs into, if any.
#[derive(Clone, Copy, Debug)]
pub struct Flight {
    block: Option<(DVec3, Pos)>,
    end: DVec3,
    struck: bool,
}

impl Flight {
    /// Whether it ended at what it struck (it does not pierce on).
    pub fn stopped(&self) -> bool {
        self.struck
    }
}

/// `EntityTypes.ARROW` and `TRIDENT`: half a block each way.
pub const SIZE: f64 = 0.5;

/// `Projectile.lerpRotation`: a fifth of the way, the short way round.
fn lerp_rotation(mut from: f32, to: f32) -> f32 {
    while to - from < -180.0 {
        from -= 360.0;
    }
    while to - from >= 180.0 {
        from += 360.0;
    }
    from + 0.2 * (to - from)
}

/// `atan2` in degrees, as `Projectile` turns it to a float.
fn degrees(y: f64, x: f64) -> f32 {
    (crate::control::minecraft_atan2(y, x) * minecraftoss_player::mth::RAD_TO_DEG) as f32
}

impl Arrow {
    fn new(position: DVec3, velocity: DVec3, random: LegacyRandom) -> Self {
        let mut arrow = Self {
            position,
            velocity,
            in_ground: false,
            in_ground_time: 0,
            life: 0,
            shake_time: 0,
            alive: true,
            impact_block: None,
            base_damage: 2.0,
            kind: ArrowKind::Arrow,
            crit: false,
            pierce_level: 0,
            pickup: Pickup::Disallowed,
            item: None,
            weapon: None,
            fire_ticks: 0,
            in_water: false,
            no_physics: false,
            left_owner: false,
            yaw: 0.0,
            pitch: 0.0,
            sound: ArrowKind::Arrow.hit_ground_sound(),
            dealt_damage: false,
            loyalty: 0,
            return_ticks: 0,
            pierced: Vec::new(),
            impact_state: None,
            random,
            hit_sound: None,
        };
        arrow.face_motion();
        arrow
    }

    /// The same arrow as another kind, with that kind's sound.
    pub fn with_kind(mut self, kind: ArrowKind) -> Self {
        self.kind = kind;
        self.sound = kind.hit_ground_sound();
        self
    }

    /// `Projectile.shoot`'s rotation: straight along its motion.
    fn face_motion(&mut self) {
        let v = self.velocity;
        self.yaw = degrees(v.x, v.z);
        self.pitch = degrees(v.y, (v.x * v.x + v.z * v.z).sqrt());
    }

    /// AbstractArrow.setBaseDamageFromMob with Normal difficulty's ID 2 in
    /// the measured skeleton fixture. The damage stream precedes shoot's
    /// independent stream reset in the harness.
    pub fn set_base_damage_from_mob(
        &mut self,
        power: f32,
        difficulty_id: u32,
        random: &mut LegacyRandom,
    ) {
        // `power * 2.0F + random.triangle(difficulty * 0.11, 0.57425)`, the
        // triangle (`mode + deviation * (a - b)`) summed on its own first.
        let mode = f64::from(difficulty_id) * 0.11;
        let triangle = mode + 0.57425 * (random.next_double() - random.next_double());
        self.base_damage = f64::from(power * 2.0) + triangle;
    }

    /// An arrow in flight at `velocity` (`summon` with `Motion`), with its
    /// default base damage.
    pub fn in_flight(position: DVec3, velocity: DVec3, random: LegacyRandom) -> Self {
        Self::new(position, velocity, random)
    }

    pub fn shoot(
        position: DVec3,
        direction: DVec3,
        power: f32,
        uncertainty: f32,
        random: &mut LegacyRandom,
    ) -> Self {
        let length_squared =
            direction.x * direction.x + direction.y * direction.y + direction.z * direction.z;
        let unit = if length_squared < 1.0e-8 {
            DVec3::ZERO
        } else {
            let length = length_squared.sqrt();
            DVec3::new(
                direction.x / length,
                direction.y / length,
                direction.z / length,
            )
        };
        let spread = 0.0172275 * uncertainty as f64;
        let velocity = DVec3::new(
            unit.x + (random.next_double() - random.next_double()) * spread,
            unit.y + (random.next_double() - random.next_double()) * spread,
            unit.z + (random.next_double() - random.next_double()) * spread,
        ) * power as f64;
        Self::new(position, velocity, random.clone())
    }

    /// `Projectile.shootFromRotation`: along the look (`x_rot` pitch,
    /// `y_rot` yaw, the pitch raised by `y_offset`) at `power`, carrying
    /// the shooter's known motion (none upward while it stands).
    #[allow(clippy::too_many_arguments)]
    pub fn shoot_from_rotation(position: DVec3, x_rot: f32, y_rot: f32, y_offset: f32, power: f32, uncertainty: f32, random: &mut LegacyRandom, motion: DVec3, on_ground: bool) -> Self {
        use minecraftoss_player::mth::{cos, sin};
        let rad = |degrees: f32| f64::from(degrees * (std::f32::consts::PI / 180.0));
        let direction = DVec3::new(
            f64::from(-sin(rad(y_rot)) * cos(rad(x_rot))),
            f64::from(-sin(rad(x_rot + y_offset))),
            f64::from(cos(rad(y_rot)) * cos(rad(x_rot))),
        );
        let mut arrow = Self::shoot(position, direction, power, uncertainty, random);
        arrow.velocity += DVec3::new(motion.x, if on_ground { 0.0 } else { motion.y }, motion.z);
        arrow
    }

    /// A skeleton's ordinary bow shot at a living target. The caller supplies
    /// the target's bounding-box height and difficulty's ranged uncertainty.
    pub fn skeleton_shot(
        shooter: DVec3,
        shooter_eye_height: f32,
        target: DVec3,
        target_height: f32,
        uncertainty: f32,
        random: &mut LegacyRandom,
    ) -> Self {
        let position = DVec3::new(
            shooter.x,
            shooter.y + shooter_eye_height as f64 - 0.1_f32 as f64,
            shooter.z,
        );
        let dx = target.x - shooter.x;
        let dy = target.y + target_height as f64 * (1.0 / 3.0) - position.y;
        let dz = target.z - shooter.z;
        let horizontal = (dx * dx + dz * dz).sqrt();
        Self::shoot(
            position,
            DVec3::new(dx, dy + horizontal * 0.2_f32 as f64, dz),
            1.6,
            uncertainty,
            random,
        )
    }

    /// One server tick in open air; new arrows first tick on the following
    /// world tick, after being inserted during the shooter's tick.
    pub fn tick_open_air(&mut self) {
        if self.in_ground {
            return;
        }
        self.position += self.velocity;
        self.velocity *= 0.99_f32 as f64;
        self.velocity.y -= 0.05;
    }

    /// Its box (`makeBoundingBox`).
    pub fn bounds(&self) -> (DVec3, DVec3) {
        let half = SIZE / 2.0;
        let p = self.position;
        (DVec3::new(p.x - half, p.y, p.z - half), DVec3::new(p.x + half, p.y + SIZE, p.z + half))
    }

    /// Whether it is on fire (`isOnFire`).
    pub fn on_fire(&self) -> bool {
        self.fire_ticks > 0
    }

    /// `AbstractArrow.tick` up to `stepMoveAndHit`: the block it stands in
    /// lodges it, the shake runs down, water puts its fire out; a landed
    /// arrow falls when its block changes and despawns after a minute
    /// (`tickDespawn`; a loyal trident one may pick up stays); in flight,
    /// water's drag, the turn toward its heading, `checkLeftOwner` against
    /// the shooter's box (`owner_box`, none once it is gone) and the block
    /// its motion runs into. `None` when it does not fly this tick.
    pub fn begin_tick(&mut self, world: &impl World, owner_box: Option<(DVec3, DVec3)>, keeps: bool) -> Option<Flight> {
        if !self.alive {
            return None;
        }
        let cell = (self.position.x.floor() as i32, self.position.y.floor() as i32, self.position.z.floor() as i32);
        if !self.no_physics {
            let p = self.position;
            let inside = collision_boxes_at(world, cell).into_iter().any(|(min, max)| p.x >= min.x && p.x < max.x && p.y >= min.y && p.y < max.y && p.z >= min.z && p.z < max.z);
            if inside {
                self.velocity = DVec3::ZERO;
                self.in_ground = true;
            }
        }
        if self.shake_time > 0 {
            self.shake_time -= 1;
        }
        if self.in_water {
            self.fire_ticks = 0;
        }
        if self.in_ground && !self.no_physics {
            if world.block(cell) != self.impact_state && self.should_fall(world) {
                self.in_ground = false;
                self.velocity *= DVec3::new(
                    f64::from(self.random.next_float() * 0.2),
                    f64::from(self.random.next_float() * 0.2),
                    f64::from(self.random.next_float() * 0.2),
                );
                self.life = 0;
            } else if !keeps {
                self.life += 1;
                if self.life >= 1200 {
                    self.alive = false;
                }
            }
            self.in_ground_time += 1;
            return None;
        }
        self.in_ground_time = 0;
        if self.in_water {
            self.velocity *= f64::from(self.kind.water_inertia());
        }
        let v = self.velocity;
        let yaw = if self.no_physics { degrees(-v.x, -v.z) } else { degrees(v.x, v.z) };
        let pitch = degrees(v.y, (v.x * v.x + v.z * v.z).sqrt());
        self.pitch = lerp_rotation(self.pitch, pitch);
        self.yaw = lerp_rotation(self.yaw, yaw);
        if !self.left_owner {
            self.left_owner = owner_box.is_none_or(|(owner_min, owner_max)| {
                let (min, max) = self.bounds();
                let (min, max) = (min.min(min + v) - DVec3::ONE, max.max(max + v) + DVec3::ONE);
                !(owner_min.x < max.x && owner_max.x > min.x && owner_min.y < max.y && owner_max.y > min.y && owner_min.z < max.z && owner_max.z > min.z)
            });
        }
        if self.no_physics {
            self.position += self.velocity;
            return Some(Flight { block: None, end: self.position, struck: true });
        }
        let block = first_block_hit(world, self.position, self.velocity).map(|(_, at, pos)| (at, pos));
        let end = block.map_or(self.position + self.velocity, |(at, _)| at);
        Some(Flight { block, end, struck: false })
    }

    /// `findHitEntities` along the rest of this tick's flight: the targets
    /// it passes through (but those it pierced already), nearest by their
    /// feet first. The arrow moves to the first one's hit point; one that
    /// does not pierce stops there. The world strikes each in turn while
    /// the arrow lives, and asks again while it pierces on
    /// (`stepMoveAndHit`). The world leaves the shooter out of `targets`
    /// until it has left it.
    pub fn entity_hits(&mut self, flight: &mut Flight, targets: &[ArrowTarget]) -> Vec<ArrowImpact> {
        if self.no_physics || flight.struck || !self.alive {
            return Vec::new();
        }
        let from = self.position;
        let ray = flight.end - from;
        let mut hits: Vec<(f64, u64, DVec3)> = targets
            .iter()
            .filter(|t| !self.pierced.contains(&t.id))
            .filter_map(|t| clip_fraction(from, ray, t.min, t.max).map(|f| (from.distance_squared(t.feet), t.id, from + ray * f)))
            .collect();
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        let Some(&(_, _, at)) = hits.first() else { return Vec::new() };
        self.position = at;
        flight.struck = self.pierce_level == 0;
        let speed = f64::from(self.velocity.length() as f32);
        let damage = (speed * self.base_damage).max(0.0).ceil() as f32;
        hits.into_iter().map(|(_, target_id, _)| ArrowImpact { target_id, damage, velocity: self.velocity }).collect()
    }

    /// `onHitEntity`'s piercing count: false when the arrow has pierced as
    /// many as it may and is gone; else `id` is among those pierced.
    pub fn pierce(&mut self, id: u64) -> bool {
        if self.pierce_level == 0 {
            return true;
        }
        if self.pierced.len() >= usize::from(self.pierce_level) + 1 {
            self.alive = false;
            return false;
        }
        self.pierced.push(id);
        true
    }

    /// A hit's damage from the base damage the weapon's enchantments left
    /// (`modifyDamage`): `ceil(speed * base)`, and a critical arrow's
    /// bonus of up to half again and one (`nextInt(damage / 2 + 2)`).
    pub fn hit_damage(&mut self, base: f64) -> f32 {
        let speed = f64::from(self.velocity.length() as f32);
        let mut damage = (speed * base).clamp(0.0, 2.147483647e9).ceil() as i32;
        if self.crit {
            let bonus = self.random.next_int((damage / 2 + 2) as u32) as i64;
            damage = (bonus + i64::from(damage)).min(i64::from(i32::MAX)) as i32;
        }
        damage as f32
    }

    /// The rest of `onHitEntity` once the world has hurt the target: a hit
    /// that took plays its sound and ends the arrow (a piercing one flies
    /// on); one turned away reverses it (`ProjectileDeflection.REVERSE`,
    /// a trident's mostly upward).
    pub fn resolve_entity_hit(&mut self, accepted: bool) {
        if self.kind == ArrowKind::Trident {
            self.hit_sound = Some(("item.trident.hit", self.position, 1.0));
            self.deflect(DVec3::new(0.02, 0.2, 0.02));
            return;
        }
        if accepted {
            self.play_hit_sound();
            if self.pierce_level == 0 {
                self.alive = false;
            }
        } else {
            self.deflect(DVec3::splat(0.2));
        }
    }

    /// `ProjectileDeflection.REVERSE` at `power`: back the way it came at
    /// half the power, turned about.
    fn deflect(&mut self, power: DVec3) {
        let rotation = 170.0 + self.random.next_float() * 20.0;
        self.velocity *= -power * 0.5;
        self.yaw += rotation;
    }

    /// The end of the tick: the block it ran into when nothing stopped it
    /// first (`onHitBlock`: lodged a hair back along its motion, its
    /// sound, shaking, no longer critical or piercing), else on to the
    /// end of its motion; air drag out of water, gravity while flying,
    /// then `Entity.baseTick`'s water check, fire and the floor 64 below
    /// the world. Returns the block it struck.
    pub fn finish_tick(&mut self, world: &impl World, flight: Flight) -> Option<Pos> {
        let mut struck = None;
        if !flight.struck && self.alive {
            self.position = flight.end;
            if let Some((hit, block)) = flight.block {
                // `Math.signum`: zero stays zero.
                let signum = |v: f64| if v == 0.0 { v } else { v.signum() };
                let sign = DVec3::new(signum(self.velocity.x), signum(self.velocity.y), signum(self.velocity.z));
                self.position = hit - sign * 0.05_f32 as f64;
                self.velocity = DVec3::ZERO;
                self.play_hit_sound();
                self.in_ground = true;
                self.impact_block = Some(block);
                self.impact_state = world.block(block);
                self.shake_time = 7;
                self.crit = false;
                self.pierce_level = 0;
                self.sound = "entity.arrow.hit";
                self.pierced.clear();
                struck = Some(block);
            }
        }
        if !self.in_water {
            self.velocity *= 0.99_f32 as f64;
        }
        if !self.no_physics && !self.in_ground {
            self.velocity.y -= 0.05;
        }
        self.in_water = crate::fluid::FluidFrame::sample(world, self.position, SIZE as f32, SIZE as f32).in_water();
        if self.fire_ticks > 0 {
            self.fire_ticks -= 1;
        }
        // `Entity.checkBelowWorld`.
        if self.position.y < f64::from(world.min_y() - 64) {
            self.alive = false;
        }
        struck
    }

    /// `playSound(soundEvent, 1.0F, 1.2F / (random.nextFloat() * 0.2F +
    /// 0.9F))` where it stands, on a block or an entity it hurt.
    fn play_hit_sound(&mut self) {
        let pitch = 1.2 / (self.random.next_float() * 0.2 + 0.9);
        self.hit_sound = Some((self.sound, self.position, pitch));
    }

    /// The hit sound it played since the last call: event, where, pitch.
    pub fn take_hit_sound(&mut self) -> Option<(&'static str, DVec3, f32)> {
        self.hit_sound.take()
    }

    /// `playerTouch`: a landed arrow (or a returning trident) done
    /// shaking, whose box meets the player's grown by a block across and
    /// half up and down (`Player.aiStep`'s touch box).
    pub fn touched_by(&self, feet: DVec3) -> bool {
        if !self.alive || !(self.in_ground || self.no_physics) || self.shake_time > 0 {
            return false;
        }
        let (min, max) = self.bounds();
        let (pmin, pmax) = (feet - DVec3::new(1.3, 0.5, 1.3), feet + DVec3::new(1.3, 2.3, 1.3));
        min.x < pmax.x && max.x > pmin.x && min.y < pmax.y && max.y > pmin.y && min.z < pmax.z && max.z > pmin.z
    }

    /// `getPickupItem`: what a pickup gives.
    pub fn pickup_item(&self) -> ItemStack {
        self.item.clone().unwrap_or_else(|| ItemStack::new(self.kind.default_item(), 1))
    }

    fn should_fall(&self, world: &impl World) -> bool {
        let low = self.position - DVec3::splat(0.06);
        let high = self.position + DVec3::splat(0.06);
        let min = low.floor();
        let max = high.floor();
        for x in min.x as i32..=max.x as i32 {
            for y in min.y as i32..=max.y as i32 {
                for z in min.z as i32..=max.z as i32 {
                    for (box_min, box_max) in collision_boxes_at(world, (x, y, z)) {
                        if low.x < box_max.x
                            && high.x > box_min.x
                            && low.y < box_max.y
                            && high.y > box_min.y
                            && low.z < box_max.z
                            && high.z > box_min.z
                        {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}

fn first_block_hit(
    world: &impl World,
    origin: DVec3,
    movement: DVec3,
) -> Option<(f64, DVec3, Pos)> {
    let end = origin + movement;
    let ray = end - origin;
    let min = origin.min(end).floor();
    let max = origin.max(end).floor();
    let mut best: Option<(f64, Pos)> = None;
    for x in min.x as i32..=max.x as i32 {
        for y in min.y as i32..=max.y as i32 {
            for z in min.z as i32..=max.z as i32 {
                let pos = (x, y, z);
                for (box_min, box_max) in collision_boxes_at(world, pos) {
                    if let Some(fraction) = clip_fraction(origin, ray, box_min, box_max) {
                        if best.is_none_or(|(previous, _)| fraction < previous) {
                            best = Some((fraction, pos));
                        }
                    }
                }
            }
        }
    }
    best.map(|(fraction, pos)| (fraction, origin + ray * fraction, pos))
}

fn clip_fraction(origin: DVec3, movement: DVec3, box_min: DVec3, box_max: DVec3) -> Option<f64> {
    let mut near: f64 = 0.0;
    let mut far: f64 = 1.0;
    for axis in 0..3 {
        if (origin[axis] <= box_min[axis] && movement[axis] < 0.0)
            || (origin[axis] >= box_max[axis] && movement[axis] > 0.0)
        {
            return None;
        }
        if movement[axis] == 0.0 {
            if origin[axis] < box_min[axis] || origin[axis] > box_max[axis] {
                return None;
            }
        } else {
            let a = (box_min[axis] - origin[axis]) / movement[axis];
            let b = (box_max[axis] - origin[axis]) / movement[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
    }
    (near <= far && (0.0..=1.0).contains(&near)).then_some(near)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_normal_skeleton_bow_open_air_trace() {
        // scenarios/mobs/skeleton-player-bow.json; 26.3, shoot seed 881.
        let mut random = LegacyRandom::new(881);
        let mut arrow = Arrow::skeleton_shot(
            DVec3::new(4.7811648174303185, 1.0, 4.53125),
            1.74,
            DVec3::new(14.5, 1.0, 4.5),
            1.8,
            6.0,
            &mut random,
        );
        assert_eq!(arrow.position.y.to_bits(), 2.640000008046627_f64.to_bits());
        assert_eq!(arrow.velocity.x.to_bits(), 1.6466665306024024_f64.to_bits());
        assert_eq!(
            arrow.velocity.y.to_bits(),
            0.25545528392154754_f64.to_bits()
        );
        assert_eq!(
            arrow.velocity.z.to_bits(),
            0.03761536055610391_f64.to_bits()
        );
        arrow.tick_open_air();
        assert_eq!(arrow.position.x.to_bits(), 6.427831348032721_f64.to_bits());
        assert_eq!(arrow.position.y.to_bits(), 2.8954552919681746_f64.to_bits());
        assert_eq!(
            arrow.velocity.y.to_bits(),
            0.20290073351854349_f64.to_bits()
        );
        arrow.set_base_damage_from_mob(1.0, 2, &mut LegacyRandom::new(419));
        assert_eq!(
            arrow.base_damage.to_bits(),
            2.4331762215570514_f64.to_bits()
        );
    }
}
