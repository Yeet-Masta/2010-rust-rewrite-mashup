//! Arrows in the entity world, and what a player's bow and crossbow loose
//! (26.3 `ProjectileWeaponItem.shoot`, `BowItem.shootProjectile`,
//! `CrossbowItem.shootProjectile`/`getShotPitch`,
//! `ProjectileWeaponItem.createProjectile`, `ArrowItem.createArrow`,
//! `Projectile.applyOnProjectileSpawned`, `AbstractArrow.tick`'s
//! `stepMoveAndHit`, `onHitEntity`, `doKnockback`,
//! `Arrow.doPostHurtEffects`, `AbstractArrow.playerTouch`/`tryPickup`).
//! A player's arrow strikes with its weapon's enchantments (power's damage,
//! punch's knockback, flame's fire); a critical one adds a random bonus; a
//! tipped one leaves its potion's effects at an eighth of their time; a
//! piercing one passes through as many as its level and one. A landed
//! arrow is picked up by the player who touches it when it may be.
use super::*;
use crate::projectile::{ArrowKind, Pickup};
use minecraftoss_player::enchantment::{DamageFacts, EntityEffect, EntityFacts, PlayerFacts, Slot};

/// A volley a player's bow or crossbow looses (`ProjectileWeaponItem.shoot`).
#[derive(Clone, Debug)]
pub struct PlayerShot {
    pub player_id: u64,
    /// The eyes: the arrows start 0.1 below them.
    pub eye: DVec3,
    /// The look's pitch and yaw (`getXRot`, `getYRot`).
    pub x_rot: f32,
    pub y_rot: f32,
    /// `getKnownMovement`, and whether the shooter stands (a bow's arrows
    /// carry it, all but its rise while standing).
    pub motion: DVec3,
    pub on_ground: bool,
    /// The weapon as it was when it shot.
    pub weapon: ItemStack,
    /// What it drew (`draw`), each one arrow; those marked
    /// `minecraft:intangible_projectile` came free (infinity, creative,
    /// multishot's copies) and only a creative player picks them up.
    pub projectiles: Vec<ItemStack>,
    pub power: f32,
    pub uncertainty: f32,
    pub crit: bool,
    /// A crossbow's shot (`CrossbowItem.shootProjectile`), else a bow's.
    pub crossbow: bool,
    /// The shooter's game mode, for the enchantments' conditions.
    pub game_mode: &'static str,
}

/// `Entity.calculateViewVector` in floats.
fn view_vector(x_rot: f32, y_rot: f32) -> glam::Vec3 {
    use minecraftoss_player::mth::{cos, sin};
    let rad = std::f32::consts::PI / 180.0;
    let (x, y) = (f64::from(x_rot * rad), f64::from(-y_rot * rad));
    glam::Vec3::new(sin(y) * cos(x), -sin(x), cos(y) * cos(x))
}

/// A player's box at its feet.
fn player_box(feet: DVec3) -> (DVec3, DVec3) {
    (feet - DVec3::new(0.3, 0.0, 0.3), feet + DVec3::new(0.3, 1.8, 0.3))
}

/// A mob's box.
fn body_box(body: &Body) -> (DVec3, DVec3) {
    let half = f64::from(body.width / 2.0);
    let p = body.position;
    (DVec3::new(p.x - half, p.y, p.z - half), DVec3::new(p.x + half, p.y + f64::from(body.height), p.z + half))
}

impl EntityWorld {
    /// `ProjectileWeaponItem.shoot`: the weapon's spread (multishot's ten
    /// degrees) fans the projectiles out from the middle, each one an
    /// arrow (`createProjectile`: critical as asked, a crossbow's with its
    /// own hit sound) from the eyes less 0.1, shot along the look turned
    /// by its angle (a bow's by yaw, a crossbow's about the up vector) with
    /// its own random's spread; the weapon's piercing and the spawned
    /// effects (flame's fire) take hold, and a free one is only a creative
    /// player's to take back. The enchantments draw on the level's random.
    pub fn shoot_player(&mut self, shot: &PlayerShot) {
        let effects = self.enchantment_effects();
        let mut random = std::mem::take(&mut self.level_random);
        let shooter = EntityFacts {
            type_id: "minecraft:player".to_owned(),
            position: shot.eye - DVec3::new(0.0, 1.62, 0.0),
            width: 0.6,
            height: 1.8,
            movement: shot.motion,
            on_ground: shot.on_ground,
            player: Some(PlayerFacts { game_mode: shot.game_mode.to_owned(), food_level: 20 }),
            main_hand: Some(shot.weapon.clone()),
            ..EntityFacts::default()
        };
        let count = shot.projectiles.len();
        let max_angle = effects.projectile_spread(&shot.weapon, &shooter, 0.0, &mut random);
        let step = if count == 1 { 0.0 } else { 2.0 * max_angle / (count - 1) as f32 };
        let offset = ((count.max(1) - 1) % 2) as f32 * step / 2.0;
        let mut direction = 1.0;
        for (index, projectile) in shot.projectiles.iter().enumerate() {
            let angle = offset + direction * ((index + 1) / 2) as f32 * step;
            direction = -direction;
            let seed = self.projectile_seed_random.next_long();
            let mut arrow_random = LegacyRandom::new(seed);
            let start = DVec3::new(shot.eye.x, shot.eye.y - f64::from(0.1_f32), shot.eye.z);
            let mut arrow = if shot.crossbow {
                // The look turned `angle` degrees about `getUpVector`.
                let up = view_vector(shot.x_rot - 90.0, shot.y_rot);
                let turn = glam::Quat::from_axis_angle(up.normalize_or_zero(), angle * (std::f32::consts::PI / 180.0));
                let along = turn * view_vector(shot.x_rot, shot.y_rot);
                Arrow::shoot(start, along.as_dvec3(), shot.power, shot.uncertainty, &mut arrow_random)
            } else {
                Arrow::shoot_from_rotation(start, shot.x_rot, shot.y_rot + angle, 0.0, shot.power, shot.uncertainty, &mut arrow_random, shot.motion, shot.on_ground)
            };
            let kind = if projectile.id == "minecraft:spectral_arrow" { ArrowKind::Spectral } else { ArrowKind::Arrow };
            arrow = arrow.with_kind(kind);
            arrow.crit = shot.crit;
            if shot.crossbow {
                arrow.sound = "item.crossbow.hit";
            }
            // The `AbstractArrow` constructor: a free arrow is the
            // creative player's only; a player's own one is allowed.
            let mut item = projectile.clone();
            item.count = 1;
            let intangible = item.components.as_mut().and_then(serde_json::Value::as_object_mut).and_then(|c| c.remove("minecraft:intangible_projectile")).is_some();
            if item.components.as_ref().is_some_and(|c| c.as_object().is_some_and(serde_json::Map::is_empty)) {
                item.components = None;
            }
            arrow.pickup = if intangible { Pickup::CreativeOnly } else { Pickup::Allowed };
            arrow.pierce_level = effects.piercing_count(&shot.weapon, &item, &mut random).clamp(0, 127) as u8;
            // `applyOnProjectileSpawned`: the arrow's own enchantments, then
            // the weapon's.
            let facts = EntityFacts { type_id: kind.type_id().to_owned(), position: arrow.position, width: 0.5, height: 0.5, movement: arrow.velocity, ..EntityFacts::default() };
            let mut spawned = effects.projectile_spawned(&item, &facts, &mut random);
            if shot.weapon.id != item.id {
                spawned.extend(effects.projectile_spawned(&shot.weapon, &facts, &mut random));
            }
            for effect in spawned {
                if let EntityEffect::Ignite { seconds } = effect {
                    arrow.fire_ticks = arrow.fire_ticks.max((seconds * 20.0).floor() as i32);
                }
            }
            arrow.item = Some(item);
            arrow.weapon = Some(shot.weapon.clone());
            self.spawn_arrow(PLAYER_TARGET + shot.player_id, arrow);
        }
        self.level_random = random;
    }

    /// The living things an arrow may strike: every living mob and
    /// pickable player, its shooter only once it has flown clear.
    pub(super) fn arrow_targets(&self, owner_id: u64, players: &[PlayerCandidate], left_owner: bool) -> Vec<ArrowTarget> {
        let mut targets: Vec<ArrowTarget> = self
            .order
            .iter()
            .filter_map(|key| {
                let id = key.id();
                let (body, health) = self.mob_body(id)?;
                if (id == owner_id && !left_owner) || health <= 0.0 {
                    return None;
                }
                let (min, max) = body_box(&body);
                Some(ArrowTarget { id, min, max, feet: body.position })
            })
            .collect();
        // Players are pickable unless spectating (`canHitEntity`).
        targets.extend(players.iter().filter(|p| self.players_pickable && p.alive && !p.spectator && (left_owner || PLAYER_TARGET + p.id != owner_id)).map(|p| {
            let (min, max) = player_box(p.position);
            ArrowTarget { id: PLAYER_TARGET + p.id, min, max, feet: p.position }
        }));
        targets
    }

    /// `AbstractArrow.tick` for one arrow: its flight, each living thing
    /// it strikes in turn (`stepMoveAndHit`), and the block that stops it.
    pub(super) fn tick_arrow(&mut self, id: u64, world: &impl World, players: &[PlayerCandidate]) {
        let Some(index) = self.arrows.iter().position(|e| e.id == id) else { return };
        let owner_id = self.arrows[index].owner_id;
        let owner_box = if owner_id >= PLAYER_TARGET {
            players.iter().find(|p| p.id == owner_id - PLAYER_TARGET && p.alive).map(|p| player_box(p.position))
        } else {
            self.mob_body(owner_id).filter(|(_, health)| *health > 0.0).map(|(body, _)| body_box(&body))
        };
        let entity = &mut self.arrows[index];
        entity.previous_position = entity.arrow.position;
        if let Some(mut flight) = entity.arrow.begin_tick(&*world, owner_box, false) {
            loop {
                let left = self.arrows[index].arrow.left_owner;
                let targets = self.arrow_targets(owner_id, players, left);
                let hits = self.arrows[index].arrow.entity_hits(&mut flight, &targets);
                if hits.is_empty() {
                    break;
                }
                for hit in hits {
                    if !self.arrows[index].arrow.alive {
                        break;
                    }
                    self.strike(index, hit, players);
                }
                if flight.stopped() || !self.arrows[index].arrow.alive {
                    break;
                }
            }
            self.arrows[index].arrow.finish_tick(&*world, flight);
        }
        // Its hit sound, from the arrow (`SoundSource.NEUTRAL`).
        if let Some((event, position, pitch)) = self.arrows[index].arrow.take_hit_sound() {
            self.sounds.push(MobSound { event: event.to_owned(), position, volume: 1.0, pitch, category: "friendly_volume" });
        }
    }

    /// `AbstractArrow.onHitEntity` on one struck target: the damage (the
    /// weapon's `modifyDamage` on the base, the piercing count, a critical
    /// bonus), then the hurt; a hit that takes knocks back with punch and
    /// leaves a tipped or spectral arrow's effects.
    fn strike(&mut self, index: usize, hit: ArrowImpact, players: &[PlayerCandidate]) {
        let owner_id = self.arrows[index].owner_id;
        let target = hit.target_id;
        let arrow = &self.arrows[index].arrow;
        let (kind, weapon, base, on_fire) = (arrow.kind, arrow.weapon.clone(), arrow.base_damage, arrow.on_fire());
        let effects = self.enchantment_effects();
        let arrow_facts = EntityFacts { type_id: kind.type_id().to_owned(), position: arrow.position, width: 0.5, height: 0.5, movement: arrow.velocity, on_fire, ..EntityFacts::default() };
        let player_owner = owner_id.checked_sub(PLAYER_TARGET);
        let owner_facts = match player_owner {
            Some(_) => EntityFacts { type_id: "minecraft:player".to_owned(), ..EntityFacts::default() },
            None => self.mob_facts(owner_id).unwrap_or_default(),
        };
        let victim = match target.checked_sub(PLAYER_TARGET) {
            Some(player) => {
                let feet = players.iter().find(|p| p.id == player).map_or(DVec3::ZERO, |p| p.position);
                EntityFacts { type_id: "minecraft:player".to_owned(), position: feet, width: 0.6, height: 1.8, ..EntityFacts::default() }
            }
            None => self.mob_facts(target).unwrap_or_default(),
        };
        let damage_type = if kind == ArrowKind::Trident { "minecraft:trident" } else { "minecraft:arrow" };
        let source = DamageFacts { damage_type, attacker: Some(&owner_facts), direct: Some(&arrow_facts), is_direct: false };
        let mut random = std::mem::take(&mut self.level_random);
        let base = weapon.as_ref().map_or(base, |w| f64::from(effects.damage(w, &victim, &source, base as f32, &mut random)));
        self.level_random = random;
        let arrow = &mut self.arrows[index].arrow;
        if !arrow.pierce(target) {
            return;
        }
        let damage = arrow.hit_damage(base);
        let velocity = arrow.velocity;
        let position = arrow.position;
        let pierces = arrow.pierce_level > 0;
        let tipped = arrow_effects(arrow);
        let accepted = if let Some(player_id) = target.checked_sub(PLAYER_TARGET) {
            // `Player.hurtServer` turns arrows away from players who cannot
            // be hurt (creative).
            let accepted = players.iter().find(|p| p.id == player_id).is_some_and(|p| p.attackable);
            if accepted {
                let effect = self.arrows[index].effect.or_else(|| tipped.first().map(|e| (e.effect, e.duration.max(0) as u32)));
                self.player_hits.push(PlayerHit {
                    player_id,
                    damage,
                    kind: PlayerHitKind::Arrow { velocity, effect, position, pierces },
                    source: player_owner.is_none().then_some(owner_id),
                });
            }
            accepted
        } else if let Some(player) = player_owner {
            self.player_arrow_hit(index, target, damage, velocity, player, damage_type, &victim, &source, &tipped)
        } else {
            // `arrow(this, owner != null ? owner : this)`.
            let attacker = self.entity_type(owner_id).unwrap_or("minecraft:arrow");
            let result = self.apply_arrow_hit(ArrowImpact { damage, ..hit }, owner_id);
            if let Some(result) = result {
                self.credit(target, result, attacker, true);
            }
            result.is_some_and(|r| r.applied)
        };
        self.arrows[index].arrow.resolve_entity_hit(accepted);
    }

    /// A player's arrow on a mob: fire first (`igniteForSeconds(5)`, kept
    /// only if the hit takes), the hurt with the player behind it, then
    /// punch's push along the flight (`doKnockback`), the weapon's
    /// post-attack effects and the arrow's own (`doPostHurtEffects`).
    /// Endermen dodge it. Whether it took.
    #[allow(clippy::too_many_arguments)]
    fn player_arrow_hit(&mut self, index: usize, target: u64, damage: f32, velocity: DVec3, player: u64, damage_type: &'static str, victim: &EntityFacts, source: &DamageFacts, tipped: &[minecraftoss_player::effects::EffectInstance]) -> bool {
        if let Some(entity) = self.endermen.iter_mut().find(|e| e.id == target) {
            entity.dodge_pending = true;
            return false;
        }
        let weapon = self.arrows[index].arrow.weapon.clone();
        let on_fire = self.arrows[index].arrow.on_fire();
        let fire_before = self.body_random_mut(target).map(|(body, _, _)| body.fire_ticks);
        if on_fire {
            if let Some((body, _, _)) = self.body_random_mut(target) {
                body.fire_ticks = body.fire_ticks.max(100);
            }
        }
        let attack = PlayerAttack { player_id: player, weapon: None, ..PlayerAttack::default() };
        let Some(hit) = self.hurt_by_player(target, damage, &attack, damage_type, Some(velocity), &mut damage_after_armor) else { return false };
        if !hit.applied {
            if let (Some(ticks), Some((body, _, _))) = (fire_before, self.body_random_mut(target)) {
                body.fire_ticks = ticks;
            }
            return false;
        }
        let effects = self.enchantment_effects();
        let mut random = std::mem::take(&mut self.level_random);
        if let Some(weapon) = weapon.as_ref() {
            let knockback = f64::from(effects.knockback(weapon, victim, source, 0.0, &mut random));
            if knockback > 0.0 {
                if let Some((body, _, resistance)) = self.body_random_mut(target) {
                    let flat = DVec3::new(velocity.x, 0.0, velocity.z);
                    let length = flat.length();
                    if length >= 1.0e-4 {
                        let push = flat / length * (knockback * 0.6 * (1.0 - resistance).max(0.0));
                        if push.length_squared() > 0.0 {
                            body.velocity += DVec3::new(push.x, 0.1, push.z);
                            body.needs_sync = true;
                        }
                    }
                }
            }
        }
        let gear = self.mob_equipment(target);
        let gear: Vec<(Slot, &ItemStack)> = gear.iter().map(|(slot, stack)| (*slot, stack)).collect();
        let after = effects.post_attack(&gear, weapon.as_ref(), victim, source, &mut random);
        self.level_random = random;
        let mut ignored = Vec::new();
        self.apply_post_attack(target, after, &mut ignored);
        for effect in tipped {
            if let Some(kind) = crate::effects::MobEffect::from_id(&effect.effect.id()) {
                self.add_mob_effect(target, crate::effects::EffectInstance::new(kind, effect.duration, effect.amplifier));
            }
        }
        true
    }

    /// `playerTouch` for every landed arrow the player touches, done
    /// shaking: one it may pick up goes to `take` (`Inventory.add`, true
    /// when it all went in), a creative-only one is simply gone to a
    /// creative player. Returns the stacks taken, with where they lay.
    pub fn pick_up_arrows(&mut self, player_id: u64, feet: DVec3, creative: bool, mut take: impl FnMut(&ItemStack) -> bool) -> Vec<(DVec3, ItemStack)> {
        let mut taken = Vec::new();
        for entity in &mut self.arrows {
            let arrow = &mut entity.arrow;
            if !arrow.touched_by(feet) {
                continue;
            }
            let owned = entity.owner_id == PLAYER_TARGET + player_id;
            if arrow.kind == ArrowKind::Trident && !owned && entity.owner_id != NO_OWNER {
                continue;
            }
            let picked = match arrow.pickup {
                Pickup::Disallowed => false,
                Pickup::Allowed => {
                    let stack = arrow.pickup_item();
                    let took = take(&stack);
                    if took {
                        taken.push((arrow.position, stack));
                    }
                    took
                }
                Pickup::CreativeOnly => creative,
            };
            if picked {
                arrow.alive = false;
            }
        }
        taken
    }
}

/// An arrow without a shooter (a summoned one).
pub const NO_OWNER: u64 = u64::MAX;

/// A tipped arrow's effects (`Arrow.doPostHurtEffects`): its potion's and
/// custom ones, each lasting its `potion_duration_scale` (an eighth by
/// default) of its time, at least a tick; a spectral arrow's glowing for
/// ten seconds.
fn arrow_effects(arrow: &Arrow) -> Vec<minecraftoss_player::effects::EffectInstance> {
    use minecraftoss_player::effects::{EffectInstance, MobEffect as PlayerEffect, PotionContents};
    if arrow.kind == ArrowKind::Spectral {
        return vec![EffectInstance::new(PlayerEffect::Glowing, 200, 0)];
    }
    let Some(item) = arrow.item.as_ref() else { return Vec::new() };
    let Some(contents) = PotionContents::from_components(item.components.as_ref()) else { return Vec::new() };
    let default_scale = if item.id == "minecraft:tipped_arrow" { 0.125 } else { 1.0 };
    let scale = item.components.as_ref().and_then(|c| c["minecraft:potion_duration_scale"].as_f64()).map_or(default_scale, |s| s as f32);
    contents
        .all_effects()
        .into_iter()
        .map(|effect| {
            let mut scaled = effect.copy();
            scaled.duration = effect.map_duration(|d| ((d as f32 * scale).floor() as i32).max(1));
            scaled
        })
        .collect()
}
