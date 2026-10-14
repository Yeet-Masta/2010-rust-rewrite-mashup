//! Wind charges (26.3 `WindChargeItem.use`, `WindCharge`,
//! `AbstractWindCharge`, `AbstractHurtingProjectile.tick`) and the wind's
//! blasts on the mobs and players (`ServerExplosion.hurtEntities` with
//! `AbstractWindCharge`'s calculator: no harm, a push scaled by the
//! knockback multiplier). The level carries out the rest of a blast (the
//! blocks it works, the items it pushes, what the clients see) from
//! [`Spawned::wind`].
use super::*;
use crate::potion::{shot_velocity, PotionTarget};

/// A wind charge in flight (0.3125 across, its box 0.15 below it).
#[derive(Clone, Debug)]
pub struct WindChargeEntity {
    pub id: u64,
    /// Who threw it (`getOwner`).
    pub owner: Option<u64>,
    pub position: DVec3,
    pub velocity: DVec3,
    /// Where it was a tick ago, for drawing between ticks.
    pub previous_position: DVec3,
    pub tick_count: i32,
    /// `leftOwner`: it has flown clear of its thrower.
    left_owner: bool,
    pub alive: bool,
}

/// A wind blast for the level to carry out: its centre, radius, push
/// (`getKnockbackMultiplier`) and sound.
#[derive(Clone, Copy, Debug)]
pub struct WindBlast {
    pub center: DVec3,
    pub radius: f32,
    pub knockback: f32,
    pub sound: &'static str,
}

/// A wind blast's sound by its id, as a blast keeps it: a wind charge's
/// (the mace's wind burst plays it too), else a breeze's.
pub fn wind_sound(id: &str) -> &'static str {
    match id.trim_start_matches("minecraft:") {
        "entity.wind_charge.wind_burst" => "minecraft:entity.wind_charge.wind_burst",
        _ => "minecraft:entity.breeze.wind_burst",
    }
}

/// `EntityType.WIND_CHARGE`'s size.
const SIZE: f64 = 0.3125;

impl WindChargeEntity {
    /// `AbstractWindCharge.makeBoundingBox`.
    fn bounds(position: DVec3) -> (DVec3, DVec3) {
        let half = SIZE / 2.0;
        let bottom = position.y - f64::from(0.15_f32);
        (DVec3::new(position.x - half, bottom, position.z - half), DVec3::new(position.x + half, bottom + SIZE, position.z + half))
    }
}

impl EntityWorld {
    /// The wind charges in flight.
    pub fn wind_charges(&self) -> &[WindChargeEntity] {
        &self.wind_charges
    }

    /// `WindChargeItem.use`: a charge from the player's feet at the height
    /// of its eyes (`new WindCharge(player, level, x, eyeY, z)`), along the
    /// look at 1.5 with an uncertainty of 1 (`shootFromRotation`),
    /// carrying the thrower's motion.
    pub fn throw_wind_charge(&mut self, player_id: u64, feet: DVec3, eye_y: f64, x_rot: f32, y_rot: f32, motion: DVec3) {
        use minecraftoss_player::mth::{cos, sin};
        let rad = |degrees: f32| f64::from(degrees * (std::f32::consts::PI / 180.0));
        let (x, y) = (rad(x_rot), rad(y_rot));
        let direction = DVec3::new(f64::from(-sin(y) * cos(x)), f64::from(-sin(x)), f64::from(cos(y) * cos(x)));
        let seed = self.projectile_seed_random.next_long();
        let mut random = LegacyRandom::new(seed);
        let velocity = shot_velocity(direction, 1.5, 1.0, &mut random) + motion;
        self.next_id += 1;
        let position = DVec3::new(feet.x, eye_y, feet.z);
        self.wind_charges.push(WindChargeEntity { id: self.next_id, owner: Some(PLAYER_TARGET + player_id), position, velocity, previous_position: position, tick_count: 0, left_owner: false, alive: true });
    }

    /// `AbstractHurtingProjectile.tick` for each charge (no drag, no
    /// pull): the hit along its motion, the move, then the hit's burst.
    /// A struck mob takes 1 (`wind_charge`) and the burst is where the
    /// charge met it; a struck block's burst is a quarter block out from
    /// its face. One that climbs 30 above the world bursts there.
    pub fn tick_wind_charges(&mut self, world: &impl World, players: &[PlayerCandidate]) {
        for (blast, impulse, skip) in std::mem::take(&mut self.pending_wind) {
            self.wind_blast(world, blast, impulse, skip, players);
        }
        for index in 0..self.wind_charges.len() {
            let mut charge = self.wind_charges[index].clone();
            charge.previous_position = charge.position;
            charge.tick_count += 1;
            if charge.position.y.floor() as i32 > world.max_y() + 30 {
                charge.alive = false;
                self.wind_charges[index] = charge.clone();
                self.wind_charge_burst(world, charge.position, players);
                continue;
            }
            let targets = self.potion_targets(charge.owner, players);
            let hit = wind_hit(world, &charge, &targets);
            charge.position = match hit {
                Some((at, _)) => at,
                None => charge.position + charge.velocity,
            };
            // `Projectile.checkLeftOwner`.
            if !charge.left_owner {
                let owner_box = charge.owner.and_then(|o| o.checked_sub(PLAYER_TARGET)).and_then(|p| players.iter().find(|c| c.id == p && c.alive)).map(|p| (p.position - DVec3::new(0.3, 0.0, 0.3), p.position + DVec3::new(0.3, 1.8, 0.3)));
                charge.left_owner = owner_box.is_none_or(|(owner_min, owner_max)| {
                    let (min, max) = WindChargeEntity::bounds(charge.position);
                    let (min, max) = (min.min(min + charge.velocity) - DVec3::ONE, max.max(max + charge.velocity) + DVec3::ONE);
                    !(owner_min.x < max.x && owner_max.x > min.x && owner_min.y < max.y && owner_max.y > min.y && owner_min.z < max.z && owner_max.z > min.z)
                });
            }
            let Some((at, struck)) = hit else {
                self.wind_charges[index] = charge;
                continue;
            };
            charge.alive = false;
            let owner = charge.owner;
            let velocity = charge.velocity;
            self.wind_charges[index] = charge;
            let center = match struck {
                WindStruck::Entity(target) => {
                    // `AbstractWindCharge.onHitEntity`.
                    if let Some(player) = owner.and_then(|o| o.checked_sub(PLAYER_TARGET)) {
                        if target < PLAYER_TARGET {
                            let attack = super::combat::PlayerAttack { player_id: player, position: at, ..super::combat::PlayerAttack::default() };
                            let _ = self.hurt_by_player(target, 1.0, &attack, "minecraft:wind_charge", Some(velocity), &mut crate::health::damage_after_armor);
                        }
                    }
                    at
                }
                WindStruck::Block(normal) => at + normal * 0.25,
            };
            self.wind_charge_burst(world, center, players);
        }
        self.wind_charges.retain(|c| c.alive);
    }

    /// A wind blast to carry out with the next tick's.
    pub fn queue_wind_blast(&mut self, blast: WindBlast, impulse: bool, skip: Option<u64>) {
        self.pending_wind.push((blast, impulse, skip));
    }

    /// `WindCharge.explode`: a burst of 1.2 pushing by 1.22, with
    /// `entity.wind_charge.wind_burst`.
    fn wind_charge_burst(&mut self, world: &impl World, center: DVec3, players: &[PlayerCandidate]) {
        self.wind_blast(world, WindBlast { center, radius: 1.2, knockback: 1.22, sound: "minecraft:entity.wind_charge.wind_burst" }, true, None, players);
    }

    /// A wind blast (`ServerExplosion` with a wind calculator): every mob
    /// and player within twice its radius is pushed away from its centre
    /// by how near it is and how much of it the blast sees, times the
    /// knockback multiplier; players (but `skip`, already pushed) get it as
    /// hits, sparing their fall if `impulse`. The level gets the blast for
    /// its blocks and the clients.
    pub fn wind_blast(&mut self, world: &impl World, blast: WindBlast, impulse: bool, skip: Option<u64>, players: &[PlayerCandidate]) {
        let reach = f64::from(blast.radius * 2.0);
        if reach < 1.0e-5 {
            return;
        }
        let push = |body: &Body, eye_height: f32| {
            let distance = body.position.distance(blast.center) / reach;
            if distance > 1.0 {
                return None;
            }
            let (direction, exposure) = explosion_exposure(world, body, eye_height, blast.center);
            Some(direction * ((1.0 - distance) * f64::from(exposure) * f64::from(blast.knockback)))
        };
        for key in self.order.clone() {
            let Some((body, eye_height)) = self.blast_target(key) else { continue };
            let Some(knockback) = push(&body, eye_height) else { continue };
            if let Some((body, _, _)) = self.body_random_mut(key.id()) {
                body.velocity += knockback;
                body.needs_sync = true;
            }
        }
        for player in players.iter().filter(|p| p.alive && !p.spectator && Some(p.id) != skip) {
            let height = match player.eye_height {
                e if e >= 1.5 => 1.8,
                e if e >= 1.0 => 1.5,
                _ => 0.6,
            };
            let body = Body::new(player.position, 0.6, height);
            if let Some(knockback) = push(&body, player.eye_height) {
                self.player_hits.push(PlayerHit { player_id: player.id, damage: 0.0, kind: PlayerHitKind::Wind { knockback, impulse }, source: None });
            }
        }
        self.spawned.wind.push(blast);
    }
}

/// What a wind charge struck: a target, or a block's face (its normal).
#[derive(Clone, Copy, Debug)]
enum WindStruck {
    Entity(u64),
    Block(DVec3),
}

/// `ProjectileUtil.getHitResultOnMoveVector` for a charge: the nearest
/// target its box (grown by `computeMargin`) meets along this tick's
/// motion before any block, else the block, with where it struck.
fn wind_hit(world: &impl World, charge: &WindChargeEntity, targets: &[PotionTarget]) -> Option<(DVec3, WindStruck)> {
    let from = charge.position;
    let mut to = from + charge.velocity;
    let block = crate::sight::clip(world, from, to);
    if let Some((_, at)) = block {
        to = at;
    }
    let margin = f64::from(0.0_f32.max(0.3_f32.min((charge.tick_count - 2) as f32 / 20.0)));
    let (min, max) = WindChargeEntity::bounds(from);
    let (search_min, search_max) = (min.min(min + charge.velocity) - DVec3::ONE, max.max(max + charge.velocity) + DVec3::ONE);
    let mut nearest: Option<(f64, u64, DVec3)> = None;
    for target in targets.iter().filter(|t| !t.owner || charge.left_owner) {
        if !(target.min.x < search_max.x && target.max.x > search_min.x && target.min.y < search_max.y && target.max.y > search_min.y && target.min.z < search_max.z && target.max.z > search_min.z) {
            continue;
        }
        let grow = DVec3::splat(margin);
        if let Some(at) = crate::sight::clip_box(target.min - grow, target.max + grow, from, to) {
            let d = from.distance_squared(at);
            if nearest.is_none_or(|(best, _, _)| d < best) {
                nearest = Some((d, target.id, at));
            }
        }
    }
    if let Some((_, id, at)) = nearest {
        return Some((at, WindStruck::Entity(id)));
    }
    let (pos, at) = block?;
    if world.block(pos).is_none_or(|b| b.id == "minecraft:air") {
        return None;
    }
    // The face the hit lies on.
    let local = at - DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
    let faces = [(local.x, DVec3::NEG_X), (1.0 - local.x, DVec3::X), (local.y, DVec3::NEG_Y), (1.0 - local.y, DVec3::Y), (local.z, DVec3::NEG_Z), (1.0 - local.z, DVec3::Z)];
    let normal = faces.iter().min_by(|a, b| a.0.abs().total_cmp(&b.0.abs())).map_or(DVec3::Y, |f| f.1);
    Some((at, WindStruck::Block(normal)))
}
