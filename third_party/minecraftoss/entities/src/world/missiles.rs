//! The mobs' projectiles other than arrows, potions and wind charges
//! (26.3 `AbstractHurtingProjectile` and its fireballs and skulls,
//! `ShulkerBullet`, `LlamaSpit`): each flies by its own rule, strikes the
//! first box or block along its motion, and does its work there.
//! - A `SmallFireball` (a blaze's) burns what it hits for 5 (`fireball`,
//!   five seconds alight), or lights the struck face's block.
//! - A `LargeFireball` (a ghast's) hits for 6, then bursts with power 1,
//!   setting fires where mobs may grief.
//! - A `WitherSkull` hits for 8 and withers (ten seconds, forty on hard),
//!   then bursts with power 1.
//! - A `ShulkerBullet` homes in, hits for 4 (`mob_projectile`) and lifts
//!   its victim (levitation for ten seconds).
//! - `LlamaSpit` falls and slows like a thrown item and hits for 1.
use super::*;
use crate::monster_ai::Target;
use crate::potion::PotionTarget;
use minecraftoss_player::survival::EffectKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissileKind {
    SmallFireball,
    LargeFireball,
    WitherSkull,
    ShulkerBullet,
    LlamaSpit,
}

impl MissileKind {
    /// Its entity type.
    pub fn type_id(self) -> &'static str {
        match self {
            Self::SmallFireball => "minecraft:small_fireball",
            Self::LargeFireball => "minecraft:fireball",
            Self::WitherSkull => "minecraft:wither_skull",
            Self::ShulkerBullet => "minecraft:shulker_bullet",
            Self::LlamaSpit => "minecraft:llama_spit",
        }
    }

    /// `EntityType`'s size (a cube).
    fn size(self) -> f64 {
        match self {
            Self::SmallFireball => 0.3125,
            Self::LargeFireball => 1.0,
            Self::WitherSkull => 0.3125,
            Self::ShulkerBullet => 0.3125,
            Self::LlamaSpit => 0.25,
        }
    }

    pub fn of_id(id: &str) -> Option<Self> {
        [Self::SmallFireball, Self::LargeFireball, Self::WitherSkull, Self::ShulkerBullet, Self::LlamaSpit].into_iter().find(|k| k.type_id() == id)
    }
}

/// A mob's projectile in flight.
#[derive(Clone, Debug)]
pub struct MissileEntity {
    pub id: u64,
    pub kind: MissileKind,
    /// Who loosed it (`getOwner`).
    pub owner: Option<u64>,
    pub position: DVec3,
    pub previous_position: DVec3,
    pub velocity: DVec3,
    pub tick_count: i32,
    pub alive: bool,
    /// A shulker bullet's quarry.
    pub target: Option<Target>,
}

impl MissileEntity {
    fn bounds(&self, position: DVec3) -> (DVec3, DVec3) {
        let half = self.kind.size() / 2.0;
        (DVec3::new(position.x - half, position.y, position.z - half), DVec3::new(position.x + half, position.y + self.kind.size(), position.z + half))
    }
}

/// What a missile struck.
enum Struck {
    Entity(u64),
    Block((i32, i32, i32), DVec3),
}

/// `ProjectileUtil.getHitResultOnMoveVector`: the nearest target box along
/// this tick's motion before any block, else the block, with where.
fn missile_hit(world: &impl World, missile: &MissileEntity, targets: &[PotionTarget]) -> Option<(DVec3, Struck)> {
    let from = missile.position;
    let mut to = from + missile.velocity;
    let block = crate::sight::clip(world, from, to);
    if let Some((_, at)) = block {
        to = at;
    }
    let (min, max) = missile.bounds(from);
    let (search_min, search_max) = (min.min(min + missile.velocity) - DVec3::ONE, max.max(max + missile.velocity) + DVec3::ONE);
    let mut nearest: Option<(f64, u64, DVec3)> = None;
    // `canHitEntity`: never its owner in its first two ticks.
    for target in targets.iter().filter(|t| !t.owner || missile.tick_count > 2) {
        if !(target.min.x < search_max.x && target.max.x > search_min.x && target.min.y < search_max.y && target.max.y > search_min.y && target.min.z < search_max.z && target.max.z > search_min.z) {
            continue;
        }
        let grow = DVec3::splat(0.3);
        if let Some(at) = crate::sight::clip_box(target.min - grow, target.max + grow, from, to) {
            let d = from.distance_squared(at);
            if nearest.is_none_or(|(best, _, _)| d < best) {
                nearest = Some((d, target.id, at));
            }
        }
    }
    if let Some((_, id, at)) = nearest {
        return Some((at, Struck::Entity(id)));
    }
    let (pos, at) = block?;
    if world.block(pos).is_none_or(|b| b.id.ends_with("air")) {
        return None;
    }
    let local = at - DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
    let faces = [(local.x, DVec3::NEG_X), (1.0 - local.x, DVec3::X), (local.y, DVec3::NEG_Y), (1.0 - local.y, DVec3::Y), (local.z, DVec3::NEG_Z), (1.0 - local.z, DVec3::Z)];
    let normal = faces.iter().min_by(|a, b| a.0.abs().total_cmp(&b.0.abs())).map_or(DVec3::Y, |f| f.1);
    Some((at, Struck::Block(pos, normal)))
}

impl EntityWorld {
    /// The missiles in flight.
    pub fn missiles(&self) -> &[MissileEntity] {
        &self.missiles
    }

    /// A missile from `owner` at `position`, along `direction`: fireballs
    /// and skulls set off at their acceleration power (0.1, `assignDirectionalMovement`),
    /// spit and bullets at the speed given.
    pub fn launch_missile(&mut self, kind: MissileKind, owner: Option<u64>, position: DVec3, direction: DVec3, speed: f64, target: Option<Target>) -> u64 {
        self.next_id += 1;
        let velocity = direction.normalize_or_zero() * speed;
        self.missiles.push(MissileEntity { id: self.next_id, kind, owner, position, previous_position: position, velocity, tick_count: 0, alive: true, target });
        self.next_id
    }

    /// Each missile's tick: the hit along its motion, its move and its
    /// flight rule.
    pub(super) fn tick_missiles(&mut self, world: &impl World, players: &[PlayerCandidate]) {
        for index in 0..self.missiles.len() {
            let mut missile = self.missiles[index].clone();
            missile.previous_position = missile.position;
            missile.tick_count += 1;
            // Out of the world, or a fireball's ten seconds untouched by
            // anything (`AbstractHurtingProjectile` gives up at 1200 below
            // the build height only; a bullet or spit fades).
            if missile.position.y < f64::from(world.min_y() - 64) || missile.tick_count > 600 {
                missile.alive = false;
                self.missiles[index] = missile;
                continue;
            }
            // `ShulkerBullet.tick`: steer at its quarry's middle.
            if missile.kind == MissileKind::ShulkerBullet {
                if let Some((p, h)) = missile.target.and_then(|t| self.target_box(t, players)) {
                    let aim = p + DVec3::new(0.0, h * 0.5, 0.0) - missile.position;
                    let wanted = aim.normalize_or_zero() * 0.15;
                    missile.velocity += (wanted - missile.velocity) * 0.2;
                }
            }
            let targets = self.potion_targets(missile.owner, players);
            let hit = missile_hit(world, &missile, &targets);
            missile.position = match &hit {
                Some((at, _)) => *at,
                None => missile.position + missile.velocity,
            };
            // The flight rule, after the move.
            let in_water = world.block((missile.position.x.floor() as i32, missile.position.y.floor() as i32, missile.position.z.floor() as i32)).is_some_and(|b| b.id == "minecraft:water");
            match missile.kind {
                MissileKind::SmallFireball | MissileKind::LargeFireball | MissileKind::WitherSkull => {
                    // `getInertia` (a wither skull's 0.73, else 0.95; 0.8 in
                    // water) and the steady push along its heading.
                    let inertia = if missile.kind == MissileKind::WitherSkull { 0.73 } else if in_water { 0.8 } else { 0.95 };
                    let push = missile.velocity.normalize_or_zero() * 0.1;
                    missile.velocity = (missile.velocity + push) * inertia;
                }
                MissileKind::LlamaSpit => {
                    missile.velocity = missile.velocity * 0.99 - DVec3::new(0.0, 0.06, 0.0);
                }
                MissileKind::ShulkerBullet => {}
            }
            let Some((at, struck)) = hit else {
                self.missiles[index] = missile;
                continue;
            };
            missile.alive = false;
            let snapshot = missile.clone();
            self.missiles[index] = missile;
            self.missile_strikes(world, &snapshot, at, struck, players);
        }
        self.missiles.retain(|m| m.alive);
    }

    /// A target's feet and height, for a homing bullet.
    pub(super) fn target_box(&self, target: Target, players: &[PlayerCandidate]) -> Option<(DVec3, f64)> {
        match target {
            Target::Player(id) => players.iter().find(|p| p.id == id && p.alive).map(|p| (p.position, 1.8)),
            Target::Villager(id) | Target::Mob(id) => self.mob_body(id).filter(|(_, h)| *h > 0.0).map(|(b, _)| (b.position, f64::from(b.height))),
        }
    }

    /// `onHitEntity` and `onHitBlock`, then a fireball's or skull's burst.
    fn missile_strikes(&mut self, world: &impl World, missile: &MissileEntity, at: DVec3, struck: Struck, players: &[PlayerCandidate]) {
        let hard = self.difficulty >= 3;
        let (damage, damage_type, effect, fire): (f32, &'static str, Option<(EffectKind, u32)>, f32) = match missile.kind {
            MissileKind::SmallFireball => (5.0, "minecraft:fireball", None, 5.0),
            MissileKind::LargeFireball => (6.0, "minecraft:fireball", None, 0.0),
            MissileKind::WitherSkull => (8.0, "minecraft:wither_skull", Some((EffectKind::Wither, if hard { 800 } else { 200 })), 0.0),
            MissileKind::ShulkerBullet => (4.0, "minecraft:mob_projectile", Some((EffectKind::Levitation, 200)), 0.0),
            MissileKind::LlamaSpit => (1.0, "minecraft:spit", None, 0.0),
        };
        match struck {
            Struck::Entity(target) => {
                if let Some(player_id) = target.checked_sub(PLAYER_TARGET) {
                    self.player_hits.push(PlayerHit { player_id, damage, kind: PlayerHitKind::Projectile { velocity: missile.velocity, damage_type, effect, fire_seconds: fire }, source: missile.owner });
                } else {
                    let from = at - missile.velocity;
                    let attacker = missile.owner.unwrap_or(missile.id);
                    if self.mob_hits_mob(attacker, target, damage, from, 0.0).is_some_and(|hit| hit.applied) && fire > 0.0 {
                        if let Some(body) = self.body_mut(target) {
                            body.fire_ticks = body.fire_ticks.max((fire * 20.0) as i32);
                        }
                    }
                }
            }
            Struck::Block(pos, normal) => {
                // `SmallFireball.onHitBlock`: fire on the struck face where
                // mobs may grief.
                if missile.kind == MissileKind::SmallFireball && self.mob_griefing {
                    let n = (normal.x as i32, normal.y as i32, normal.z as i32);
                    self.spawned.fires.push((pos.0 + n.0, pos.1 + n.1, pos.2 + n.2));
                }
            }
        }
        // `LargeFireball.onHit` and `WitherSkull.onHit`: a burst of power 1
        // (the ghast's own `explosionPower`), fiery where mobs may grief.
        if matches!(missile.kind, MissileKind::LargeFireball | MissileKind::WitherSkull) {
            let blast = crate::creeper::CreeperExplosion { position: at, radius: 1.0, powered: false, fire: missile.kind == MissileKind::LargeFireball && self.mob_griefing };
            self.apply_explosion(world, missile.owner.unwrap_or(missile.id), blast, players);
            self.explosions.push(blast);
        }
        if missile.kind == MissileKind::ShulkerBullet {
            self.sounds.push(MobSound { event: "entity.shulker_bullet.hit".into(), position: at, volume: 1.0, pitch: 1.0, category: "hostile_volume" });
        }
    }
}
