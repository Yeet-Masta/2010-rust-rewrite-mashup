//! A player's melee hit on a mob (26.3 `Player.attack`, the server's half):
//! the weapon's damage scaled by the attack strength, its enchantments'
//! damage (`ServerPlayer.getEnchantedDamage`), criticals, the extra
//! knockback of sprinting and of the weapon (`getKnockback`), the sword's
//! sweep (`SWEEPING_DAMAGE_RATIO`), the mace's smash, and the weapon's
//! post-attack effects (`EnchantmentHelper.doPostAttackEffects`: fire,
//! slowness); then each mob's `hurtServer` with `player_attack` (armor,
//! less what breach takes off it, the damage cooldown, the default
//! knockback away from the player) and its reaction: animals panic
//! (`#panic_causes`), creepers remember the attacker for `HurtByTargetGoal`,
//! and zombies and skeletons turn on the player.
use super::*;
use minecraftoss_player::enchantment::{DamageFacts, EnchantmentEffects, EntityEffect, EntityFacts, PlayerFacts, PostAttack, Slot, Target};
use minecraftoss_player::item_catalog::attribute_value;
use minecraftoss_player::minecraft_sin_cos;

/// What the attacking player brings to `Player.attack`. The player runs on
/// the client, which works these out from its state and held item.
#[derive(Clone, Debug, Default)]
pub struct PlayerAttack {
    pub player_id: u64,
    /// Feet position and facing.
    pub position: DVec3,
    pub yaw: f32,
    /// `ATTACK_DAMAGE` with the main hand's modifiers.
    pub attack_damage: f64,
    /// `getAttackStrengthScale(0.5)` before the attack resets it.
    pub strength: f32,
    pub sprinting: bool,
    /// `canCriticalAttack`'s player half: falling, off the ground, not
    /// climbing, in water, riding, restricted or sprinting.
    pub can_critical: bool,
    /// `isSweepAttack`'s player half: on the ground, slower than two and a
    /// half times its speed, a sword in the main hand.
    pub can_sweep: bool,
    /// The main hand (`getWeaponItem`): its enchantments act on the hit.
    pub weapon: Option<ItemStack>,
    /// `fallDistance`, `onGround`, flying (abilities or gliding) and
    /// gliding alone: the mace's smash and the conditions read them.
    pub fall_distance: f64,
    pub on_ground: bool,
    pub flying: bool,
    pub fall_flying: bool,
    /// Its game mode (`survival`, `creative`...) and food level, for the
    /// conditions.
    pub game_mode: &'static str,
    pub food_level: i32,
}

impl PlayerAttack {
    /// The attacker as the enchantments' conditions see it.
    fn facts(&self) -> EntityFacts {
        EntityFacts {
            type_id: "minecraft:player".to_owned(),
            position: self.position,
            width: 0.6,
            height: 1.8,
            fall_distance: self.fall_distance,
            on_ground: self.on_ground,
            sprinting: self.sprinting,
            flying: self.flying,
            fall_flying: self.fall_flying,
            player: Some(PlayerFacts { game_mode: self.game_mode.to_owned(), food_level: self.food_level }),
            main_hand: self.weapon.clone(),
            ..EntityFacts::default()
        }
    }

    /// `MaceItem.canSmashAttack` with a mace in hand.
    fn smashes(&self) -> bool {
        self.weapon.as_ref().is_some_and(|w| w.id == "minecraft:mace") && self.fall_distance > 1.5 && !self.fall_flying
    }
}

/// What an attack did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttackResult {
    /// The target took damage (`hurtOrSimulate`).
    pub hurt: bool,
    pub died: bool,
    /// A full hit: outside the target's damage cooldown.
    pub full_hit: bool,
    pub full_strength: bool,
    /// Sprinting at full strength: extra knockback; the player slows and
    /// stops sprinting.
    pub knockback: bool,
    pub critical: bool,
    pub sweep: bool,
    /// The mobs the sweep hurt: ID, full hit, died.
    pub swept: Vec<(u64, bool, bool)>,
    /// The health the target lost (`damageStatsAndHearts`).
    pub damage_dealt: f32,
    /// `causeExtraKnockback` pushed the target (sprinting, or the weapon's
    /// knockback): the player slows to 60% and stops sprinting.
    pub extra_knockback: bool,
    /// The weapon's enchantments added damage (`magicCrit`'s sparks).
    pub magic: bool,
    /// The mace's smash landed (`MaceItem.hurtEnemy`): its sound, and the
    /// player's fall ends.
    pub smash: Option<&'static str>,
    /// What the weapon's post-attack effects do to the player who swung it
    /// (`affected: attacker`), and its own wear (`change_item_damage`), for
    /// the client to carry out.
    pub attacker_effects: Vec<EntityEffect>,
}

/// `LivingEntity.knockback(power, xd, zd)`: away along (xd, zd), halving
/// the motion, and up off the ground. Too small a direction is replaced by
/// a random one from the mob's own random.
fn knockback(body: &mut Body, random: &mut LegacyRandom, power: f64, mut xd: f64, mut zd: f64) {
    if power <= 0.0 {
        return;
    }
    while xd * xd + zd * zd < f64::from(1.0e-5_f32) {
        xd = (random.next_double() - random.next_double()) * 0.01;
        zd = (random.next_double() - random.next_double()) * 0.01;
    }
    let length = (xd * xd + zd * zd).sqrt();
    let (dx, dz) = (xd / length * power, zd / length * power);
    let v = body.velocity;
    let y = if body.on_ground { (v.y / 2.0 + power).min(0.4) } else { v.y };
    body.velocity = DVec3::new(v.x / 2.0 - dx, y, v.z / 2.0 - dz);
    body.needs_sync = true;
}

impl EntityWorld {
    /// The enchantments' effects, from the recipe book (none without one).
    pub(super) fn enchantment_effects(&self) -> Arc<EnchantmentEffects> {
        self.recipes.as_ref().map_or_else(|| Arc::new(EnchantmentEffects::default()), |recipes| recipes.enchantments().clone())
    }

    /// A mob as the enchantments' conditions see it.
    pub(super) fn mob_facts(&self, id: u64) -> Option<EntityFacts> {
        let (body, _) = self.mob_body(id)?;
        Some(EntityFacts {
            type_id: self.entity_type(id)?.to_owned(),
            position: body.position,
            width: body.width,
            height: body.height,
            movement: body.velocity,
            fall_distance: body.fall_distance,
            on_ground: body.on_ground,
            on_fire: body.fire_ticks > 0,
            swimming: body.swimming,
            in_water: body.touching_water,
            baby: self.zombies.iter().any(|e| e.id == id && e.zombie.baby),
            ..EntityFacts::default()
        })
    }

    /// `Player.attack` on the mob `target`. The enchantments draw on the
    /// level's random.
    pub fn player_attack(&mut self, attack: &PlayerAttack, target: u64) -> AttackResult {
        let effects = self.enchantment_effects();
        let mut random = std::mem::take(&mut self.level_random);
        let result = self.attack_with(attack, target, &effects, &mut random);
        self.level_random = random;
        result
    }

    fn attack_with(&mut self, attack: &PlayerAttack, target: u64, effects: &EnchantmentEffects, random: &mut LegacyRandom) -> AttackResult {
        let mut result = AttackResult::default();
        let Some(victim) = self.mob_facts(target) else { return result };
        let player = attack.facts();
        let weapon = attack.weapon.as_ref();
        // `getItemDamageSource`: a smashing mace deals `mace_smash`.
        let smash = attack.smashes();
        let source = DamageFacts { damage_type: if smash { "minecraft:mace_smash" } else { "minecraft:player_attack" }, attacker: Some(&player), direct: Some(&player), is_direct: true };
        // `getEnchantedDamage`: the weapon's `modifyDamage` on a victim.
        let enchanted = |victim: &EntityFacts, damage: f32, random: &mut LegacyRandom| weapon.map_or(damage, |w| effects.damage(w, victim, &source, damage, random));
        let scale = attack.strength;
        let mut base = attack.attack_damage as f32;
        let magic = scale * (enchanted(&victim, base, random) - base);
        base *= 0.2 + scale * scale * 0.8;
        if !(base > 0.0 || magic > 0.0) {
            return result;
        }
        result.full_strength = scale > 0.9;
        result.knockback = attack.sprinting && result.full_strength;
        // `getAttackDamageBonus`: the mace's smash grows with the fall, and
        // density adds its share a block fallen.
        if smash {
            let fallen = attack.fall_distance;
            let bonus = if fallen <= 3.0 {
                4.0 * fallen
            } else if fallen <= 8.0 {
                12.0 + 2.0 * (fallen - 3.0)
            } else {
                22.0 + fallen - 8.0
            };
            let density = weapon.map_or(0.0, |w| effects.fall_based_damage(w, &victim, &source, 0.0, random));
            base += (bonus + f64::from(density) * fallen) as f32;
        }
        result.critical = result.full_strength && attack.can_critical;
        if result.critical {
            base *= 1.5;
        }
        result.sweep = result.full_strength && !result.critical && !result.knockback && attack.can_sweep;
        let health = |world: &Self| world.mob_body(target).map_or(0.0, |(_, health)| health.max(0.0));
        let before = health(self);
        // `CombatRules.getDamageAfterAbsorb` with the weapon's
        // `modifyArmorEffectiveness` (breach) on the armor's share.
        let armor = |damage: f32, armor: f32, toughness: f32, victim: &EntityFacts, random: &mut LegacyRandom| {
            let toughness = 2.0 + toughness / 4.0;
            let fraction = (armor - damage / toughness).clamp(armor * 0.2, 20.0) / 25.0;
            let fraction = weapon.map_or(fraction, |w| effects.armor_effectiveness(w, victim, &source, fraction, random).clamp(0.0, 1.0));
            damage * (1.0 - fraction)
        };
        let Some(hit) = self.hurt_by_player(target, base + magic, attack, &mut |d, a, t| armor(d, a, t, &victim, random)) else { return AttackResult::default() };
        (result.hurt, result.died, result.full_hit) = (hit.applied, hit.died, hit.full);
        result.damage_dealt = before - health(self);
        if !hit.applied {
            return result;
        }
        result.magic = magic > 0.0;
        // `setLastHurtMob` at the player's tick count.
        let victim_target = if self.villagers.iter().any(|e| e.id == target) { crate::monster_ai::Target::Villager(target) } else { crate::monster_ai::Target::Mob(target) };
        let fights = self.player_fights.entry(attack.player_id).or_default();
        fights.hurt_mob = Some((victim_target, fights.tick_count));
        // `causeExtraKnockback` along the player's facing: the weapon's
        // `getKnockback` (`ATTACK_KNOCKBACK`, 0 for a player, through
        // `modifyKnockback`, halved) and the sprint's half.
        let (sin, cos) = minecraft_sin_cos(f64::from(attack.yaw));
        let mut extra = weapon.map_or(0.0, |w| effects.knockback(w, &victim, &source, 0.0, random)) / 2.0;
        if result.knockback {
            extra += 0.5;
        }
        if extra > 0.0 {
            self.knock_back(target, f64::from(extra), sin, -cos);
            result.extra_knockback = true;
        }
        if result.sweep {
            // `doSweepAttack`: 1 + SWEEPING_DAMAGE_RATIO × base, through
            // each victim's `getEnchantedDamage`, scaled.
            let ratio = weapon.map_or(0.0, |w| {
                let modifiers = effects.attribute_modifiers(w, Slot::MainHand);
                attribute_value(0.0, modifiers.iter().filter(|m| m.attribute == "minecraft:sweeping_damage_ratio"), (0.0, 1.0))
            });
            let damage = 1.0_f32 + ratio as f32 * base;
            let Some((target_body, _)) = self.mob_body(target) else { return result };
            let half = f64::from(target_body.width / 2.0);
            let p = target_body.position;
            let min = DVec3::new(p.x - half - 1.0, p.y - 0.25, p.z - half - 1.0);
            let max = DVec3::new(p.x + half + 1.0, p.y + f64::from(target_body.height) + 0.25, p.z + half + 1.0);
            for key in self.order.clone() {
                let id = key.id();
                if id == target {
                    continue;
                }
                let Some((body, _)) = self.mob_body(id) else { continue };
                let half = f64::from(body.width / 2.0);
                let q = body.position;
                let touches = q.x - half < max.x && q.x + half > min.x && q.y < max.y && q.y + f64::from(body.height) > min.y && q.z - half < max.z && q.z + half > min.z;
                if !touches || attack.position.distance_squared(q) >= 9.0 {
                    continue;
                }
                let Some(nearby) = self.mob_facts(id) else { continue };
                let amount = enchanted(&nearby, damage, random) * scale;
                if let Some(hit) = self.hurt_by_player(id, amount, attack, &mut |d, a, t| armor(d, a, t, &nearby, random)) {
                    if hit.applied {
                        self.knock_back(id, f64::from(0.4_f32), sin, -cos);
                        result.swept.push((id, hit.full, hit.died));
                        let after = effects.post_attack(&[], weapon, &nearby, &source, random);
                        self.apply_post_attack(id, after, &mut result.attacker_effects);
                    }
                }
            }
        }
        // `itemAttackInteraction`: the mace's `hurtEnemy`, then the weapon's
        // post-attack effects.
        if smash {
            result.smash = Some(if !victim.on_ground {
                "item.mace.smash_air"
            } else if attack.fall_distance > 5.0 {
                "item.mace.smash_ground_heavy"
            } else {
                "item.mace.smash_ground"
            });
            self.mace_knockback(attack, target);
        }
        let after = effects.post_attack(&[], weapon, &victim, &source, random);
        self.apply_post_attack(target, after, &mut result.attacker_effects);
        result
    }

    /// `MaceItem.knockback`: the living things within 3.5 blocks of the
    /// struck mob, but for it and the attacker, are thrown away from it and
    /// up, twice as hard after a fall of more than five blocks.
    fn mace_knockback(&mut self, attack: &PlayerAttack, target: u64) {
        let Some((struck, _)) = self.mob_body(target) else { return };
        let center = struck.position;
        let heavy = if attack.fall_distance > 5.0 { 2.0 } else { 1.0 };
        for key in self.order.clone() {
            let id = key.id();
            if id == target {
                continue;
            }
            let Some((body, _)) = self.mob_body(id) else { continue };
            let direction = body.position - center;
            if direction.length_squared() > 3.5 * 3.5 {
                continue;
            }
            let Some((body, _, resistance)) = self.body_random_mut(id) else { continue };
            let power = (3.5 - direction.length()) * f64::from(0.7_f32) * heavy * (1.0 - resistance);
            if power > 0.0 {
                let push = direction.normalize_or_zero() * power;
                body.velocity += DVec3::new(push.x, f64::from(0.7_f32), push.z);
                body.needs_sync = true;
            }
        }
    }

    /// Carries out `doPostAttack`'s effects of a hit on the mob `victim`:
    /// on the mob its status effects and fire; what lands on the attacker
    /// (or wears the weapon) goes to `attacker`.
    pub(super) fn apply_post_attack(&mut self, victim: u64, effects: Vec<PostAttack>, attacker: &mut Vec<EntityEffect>) {
        for PostAttack { affected, effect, .. } in effects {
            // The enchanted item's own wear is the attacker's to take.
            if affected != Target::Victim || matches!(effect, EntityEffect::ItemDamage { .. }) {
                attacker.push(effect);
                continue;
            }
            self.apply_entity_effect(victim, &effect);
        }
    }

    /// An enchantment's entity effect on a mob: `addEffect`, `igniteForSeconds`
    /// and its sounds; the rest has nothing to act on here.
    pub(super) fn apply_entity_effect(&mut self, id: u64, effect: &EntityEffect) {
        match effect {
            EntityEffect::MobEffect { effect, duration, amplifier } => {
                if let Some(kind) = crate::effects::MobEffect::from_id(effect) {
                    self.add_mob_effect(id, crate::effects::EffectInstance::new(kind, *duration, *amplifier));
                }
            }
            EntityEffect::Ignite { seconds } => {
                // `igniteForTicks(floor(seconds * 20))`.
                let ticks = (seconds * 20.0).floor() as i32;
                if let Some((body, _, _)) = self.body_random_mut(id) {
                    body.fire_ticks = body.fire_ticks.max(ticks);
                }
            }
            EntityEffect::Sound { event, position, volume, pitch } => {
                let event = event.strip_prefix("minecraft:").unwrap_or(event).to_owned();
                self.sounds.push(MobSound { event, position: *position, volume: *volume, pitch: *pitch, category: "hostile_volume" });
            }
            _ => {}
        }
    }

    /// `Mob.doHurtTarget` on a player with the zombie's held weapon:
    /// `ATTACK_DAMAGE` with the main hand's modifiers, the weapon's
    /// `modifyDamage`, its `getKnockback` (the zombie slows to 60% when it
    /// pushes) and its post-attack fire. The damage and what the weapon
    /// adds.
    pub(super) fn zombie_weapon_hit(&mut self, id: u64, base: f32) -> (f32, MeleeWeapon) {
        let Some(e) = self.zombies.iter().find(|e| e.id == id) else { return (base, MeleeWeapon::default()) };
        let Some(weapon) = e.zombie.equipment.iter().find(|(slot, _)| *slot == Slot::MainHand).map(|(_, stack)| stack.clone()) else { return (base, MeleeWeapon::default()) };
        let yaw = e.yaw;
        let effects = self.enchantment_effects();
        let Some(mut attacker) = self.mob_facts(id) else { return (base, MeleeWeapon::default()) };
        attacker.main_hand = Some(weapon.clone());
        let player = EntityFacts { type_id: "minecraft:player".to_owned(), ..EntityFacts::default() };
        let modifiers = self.recipes.as_ref().map_or_else(Vec::new, |recipes| recipes.item_modifiers(&weapon));
        let damage = attribute_value(f64::from(base), modifiers.iter().filter(|m| m.in_main_hand() && m.attribute == "minecraft:attack_damage"), (0.0, 2048.0)) as f32;
        let source = DamageFacts { damage_type: "minecraft:mob_attack", attacker: Some(&attacker), direct: Some(&attacker), is_direct: true };
        let mut random = std::mem::take(&mut self.level_random);
        let damage = effects.damage(&weapon, &player, &source, damage, &mut random);
        let knockback = effects.knockback(&weapon, &player, &source, 0.0, &mut random) / 2.0;
        let fire_seconds = effects
            .post_attack(&[], Some(&weapon), &player, &source, &mut random)
            .into_iter()
            .filter_map(|after| match after.effect {
                EntityEffect::Ignite { seconds } if after.affected == Target::Victim => Some(seconds),
                _ => None,
            })
            .fold(0.0, f32::max);
        self.level_random = random;
        if knockback > 0.0 {
            if let Some(e) = self.zombies.iter_mut().find(|e| e.id == id) {
                e.zombie.body.velocity.x *= 0.6;
                e.zombie.body.velocity.z *= 0.6;
            }
        }
        (damage, MeleeWeapon { knockback, yaw, fire_seconds })
    }

    /// A zombie's armor against a hit: `Zombie.createAttributes`' 2 with
    /// its worn pieces' points and toughness, and their enchantments'
    /// protection (`getDamageProtection`) against the damage type.
    pub(super) fn zombie_armor(&self, id: u64, damage_type: &str) -> (f32, f32, f32) {
        let Some(e) = self.zombies.iter().find(|e| e.id == id) else { return (2.0, 0.0, 0.0) };
        let (mut armor, mut toughness) = (2.0, 0.0);
        if let Some(recipes) = self.recipes.as_ref() {
            for (slot, stack) in &e.zombie.equipment {
                if matches!(slot, Slot::Head | Slot::Chest | Slot::Legs | Slot::Feet) {
                    armor += recipes.armor_points(stack, slot.name());
                    toughness += recipes.armor_toughness(stack, slot.name()).0;
                }
            }
        }
        let equipment: Vec<(Slot, &ItemStack)> = e.zombie.equipment.iter().map(|(slot, stack)| (*slot, stack)).collect();
        let protection = if equipment.is_empty() {
            0.0
        } else {
            let Some(victim) = self.mob_facts(id) else { return (armor as f32, toughness as f32, 0.0) };
            let source = DamageFacts { damage_type, attacker: None, direct: None, is_direct: true };
            // Protection draws nothing.
            self.enchantment_effects().damage_protection(&equipment, &victim, &source, &mut LegacyRandom::new(0))
        };
        (armor as f32, toughness as f32, protection)
    }

    /// `hurtServer` on a mob by an effect of the player's enchantments
    /// (thorns' `damage_entity`): the player at `position` is its source,
    /// so it knocks back, angers and credits a kill like a hit. Whether it
    /// took.
    pub fn hurt_by_player_effect(&mut self, id: u64, amount: f32, player_id: u64, position: DVec3) -> bool {
        let attack = PlayerAttack { player_id, position, ..PlayerAttack::default() };
        self.hurt_by_player(id, amount, &attack, &mut damage_after_armor).is_some_and(|hit| hit.applied)
    }

    /// A living mob's feet, width and height, by ID.
    pub fn mob_bounds(&self, id: u64) -> Option<(DVec3, f32, f32)> {
        self.mob_body(id).map(|(body, _)| (body.position, body.width, body.height))
    }

    /// A living mob's body and health, by ID.
    pub(super) fn mob_body(&self, id: u64) -> Option<(Body, f32)> {
        if let Some(e) = self.witches.iter().find(|e| e.id == id) {
            return Some((e.witch.body.clone(), e.witch.health));
        }
        if let Some(e) = self.bats.iter().find(|e| e.id == id) {
            return Some((e.bat.body.clone(), e.bat.health));
        }
        if let Some(e) = self.zombies.iter().find(|e| e.id == id) {
            return Some((e.zombie.body.clone(), e.zombie.health));
        }
        if let Some(e) = self.skeletons.iter().find(|e| e.id == id) {
            return Some((e.skeleton.body.clone(), e.skeleton.health));
        }
        if let Some(e) = self.creepers.iter().find(|e| e.id == id && !e.creeper.exploded) {
            return Some((e.creeper.body.clone(), e.creeper.health));
        }
        if let Some(e) = self.spiders.iter().find(|e| e.id == id) {
            return Some((e.spider.body.clone(), e.spider.health));
        }
        if let Some(e) = self.slimes.iter().find(|e| e.id == id) {
            return Some((e.slime.body.clone(), e.slime.health));
        }
        if let Some(e) = self.endermen.iter().find(|e| e.id == id) {
            return Some((e.enderman.body.clone(), e.enderman.health));
        }
        if let Some(e) = self.villagers.iter().find(|e| e.id == id) {
            return Some((e.villager.body.clone(), e.villager.health));
        }
        if let Some(e) = self.iron_golems.iter().find(|e| e.id == id) {
            return Some((e.golem.body.clone(), e.golem.health));
        }
        if let Some(e) = self.wolves.iter().find(|e| e.id == id) {
            return Some((e.wolf.body.clone(), e.wolf.health));
        }
        if let Some(e) = self.cows.iter().find(|e| e.id == id) {
            return Some((e.cow.body.clone(), e.cow.health));
        }
        if let Some(e) = self.sheep.iter().find(|e| e.id == id) {
            return Some((e.body.clone(), e.health));
        }
        if let Some(e) = self.pigs.iter().find(|e| e.id == id) {
            return Some((e.pig.body.clone(), e.pig.health));
        }
        if let Some(e) = self.chickens.iter().find(|e| e.id == id) {
            return Some((e.chicken.body.clone(), e.chicken.health));
        }
        None
    }

    /// A mob's body and random, by ID, with its `KNOCKBACK_RESISTANCE`.
    pub(super) fn body_random_mut(&mut self, id: u64) -> Option<(&mut Body, &mut LegacyRandom, f64)> {
        if let Some(e) = self.witches.iter_mut().find(|e| e.id == id) {
            Some((&mut e.witch.body, &mut e.random, 0.0))
        } else if let Some(e) = self.bats.iter_mut().find(|e| e.id == id) {
            Some((&mut e.bat.body, &mut e.random, 0.0))
        } else if let Some(e) = self.zombies.iter_mut().find(|e| e.id == id) {
            Some((&mut e.zombie.body, &mut e.random, 0.0))
        } else if let Some(e) = self.skeletons.iter_mut().find(|e| e.id == id) {
            Some((&mut e.skeleton.body, &mut e.random, 0.0))
        } else if let Some(e) = self.creepers.iter_mut().find(|e| e.id == id) {
            Some((&mut e.creeper.body, &mut e.random, 0.0))
        } else if let Some(e) = self.spiders.iter_mut().find(|e| e.id == id) {
            Some((&mut e.spider.body, &mut e.random, 0.0))
        } else if let Some(e) = self.slimes.iter_mut().find(|e| e.id == id) {
            Some((&mut e.slime.body, &mut e.random, 0.0))
        } else if let Some(e) = self.endermen.iter_mut().find(|e| e.id == id) {
            Some((&mut e.enderman.body, &mut e.random, 0.0))
        } else if let Some(e) = self.villagers.iter_mut().find(|e| e.id == id) {
            Some((&mut e.villager.body, &mut e.random, 0.0))
        } else if let Some(e) = self.iron_golems.iter_mut().find(|e| e.id == id) {
            // `IronGolem.createAttributes`: `KNOCKBACK_RESISTANCE` 1.
            Some((&mut e.golem.body, &mut e.random, 1.0))
        } else if let Some(e) = self.wolves.iter_mut().find(|e| e.id == id) {
            Some((&mut e.wolf.body, &mut e.random, 0.0))
        } else if let Some(e) = self.cows.iter_mut().find(|e| e.id == id) {
            Some((&mut e.cow.body, &mut e.random, 0.0))
        } else if let Some(e) = self.sheep.iter_mut().find(|e| e.id == id) {
            Some((&mut e.body, &mut e.random, 0.0))
        } else if let Some(e) = self.pigs.iter_mut().find(|e| e.id == id) {
            Some((&mut e.pig.body, &mut e.random, 0.0))
        } else if let Some(e) = self.chickens.iter_mut().find(|e| e.id == id) {
            Some((&mut e.chicken.body, &mut e.random, 0.0))
        } else {
            None
        }
    }

    /// `LivingEntity.knockback` on a mob, by ID: its resistance takes its
    /// share off the power.
    pub(super) fn knock_back(&mut self, id: u64, power: f64, xd: f64, zd: f64) {
        if let Some((body, random, resistance)) = self.body_random_mut(id) {
            knockback(body, random, power * (1.0 - resistance), xd, zd);
        }
    }

    /// `hurtServer` with `player_attack` on a mob: its armor, the damage
    /// cooldown, the attacker remembered, and on a full hit the default
    /// knockback away from the player. `None` when no such mob lives.
    fn hurt_by_player(&mut self, id: u64, amount: f32, attack: &PlayerAttack, armor: &mut dyn FnMut(f32, f32, f32) -> f32) -> Option<DamageResult> {
        let source = DamageSourceKind::PlayerAttack;
        let player = attack.player_id;
        let (zombie_armor, zombie_toughness, zombie_protection) = self.zombie_armor(id, "minecraft:player_attack");
        let hit = if let Some(e) = self.bats.iter_mut().find(|e| e.id == id) {
            e.hurt(amount)
        } else if let Some(e) = self.zombies.iter_mut().find(|e| e.id == id) {
            // `Zombie.createAttributes`: 2 armor, and what it wears.
            let after_armor = armor(amount, zombie_armor, zombie_toughness);
            let hit = e.hurt(minecraftoss_player::enchantment::damage_after_magic_absorb(after_armor, zombie_protection));
            if hit.applied {
                if let Some(ai) = e.ai.as_deref_mut() {
                    // `setLastHurtByMob`, for `HurtByTargetGoal`.
                    ai.state.hurt_by = Some((crate::monster_ai::Target::Player(player), e.tick_count));
                } else {
                    // The attack-only profile: its `HurtByTargetGoal` outranks
                    // the other targets.
                    e.target_player_id = Some(player);
                    e.target_villager_id = None;
                }
            }
            hit
        } else if let Some(e) = self.skeletons.iter_mut().find(|e| e.id == id) {
            let hit = e.hurt(amount);
            if hit.applied {
                if let Some(ai) = e.ai.as_deref_mut() {
                    ai.state.hurt_by = Some((crate::monster_ai::Target::Player(player), e.tick_count));
                } else {
                    e.target_player_id = Some(player);
                }
            }
            hit
        } else if let Some(e) = self.creepers.iter_mut().find(|e| e.id == id && !e.creeper.exploded) {
            let hit = e.hurt(amount);
            if hit.applied {
                // `setLastHurtByMob`, stamped with its tick count.
                e.ai.state.hurt_by = Some((crate::monster_ai::Target::Player(player), e.tick_count));
            }
            hit
        } else if let Some(e) = self.spiders.iter_mut().find(|e| e.id == id) {
            let hit = e.hurt(amount);
            if hit.applied {
                e.ai.state.hurt_by = Some((crate::monster_ai::Target::Player(player), e.tick_count));
            }
            hit
        } else if let Some(e) = self.slimes.iter_mut().find(|e| e.id == id) {
            let hit = e.hurt(amount);
            if hit.applied {
                e.ai.state.hurt_by = Some((crate::monster_ai::Target::Player(player), e.tick_count));
            }
            hit
        } else if let Some(e) = self.endermen.iter_mut().find(|e| e.id == id) {
            let hit = e.hurt(amount);
            if hit.applied {
                e.ai.state.hurt_by = Some((crate::monster_ai::Target::Player(player), e.tick_count));
            }
            hit
        } else if let Some(e) = self.witches.iter_mut().find(|e| e.id == id) {
            let hit = e.hurt(amount, false, false);
            if hit.applied {
                e.ai.state.hurt_by = Some((crate::monster_ai::Target::Player(player), e.tick_count));
            }
            hit
        } else if let Some(e) = self.villagers.iter_mut().find(|e| e.id == id) {
            e.hurt_from(amount, "minecraft:player_attack", Some(PLAYER_TARGET + player), self.game_time)
        } else if let Some(e) = self.iron_golems.iter_mut().find(|e| e.id == id) {
            let hit = e.hurt(amount);
            if hit.applied {
                e.ai.state.hurt_by = Some((crate::monster_ai::Target::Player(player), e.tick_count));
            }
            hit
        } else if let Some(e) = self.wolves.iter_mut().find(|e| e.id == id) {
            let time = self.game_time;
            e.hurt_from(amount, "minecraft:player_attack", Some(crate::monster_ai::Target::Player(player)), time)
        } else if let Some(e) = self.cows.iter_mut().find(|e| e.id == id) {
            e.hurt(amount, source)
        } else if let Some(e) = self.sheep.iter_mut().find(|e| e.id == id) {
            e.hurt(amount, source)
        } else if let Some(e) = self.pigs.iter_mut().find(|e| e.id == id) {
            e.hurt(amount, source)
        } else if let Some(e) = self.chickens.iter_mut().find(|e| e.id == id) {
            e.hurt_with_source(amount, source)
        } else {
            return None;
        };
        // `resolvePlayerResponsibleForDamage`: the player is remembered,
        // and a kill keeps the weapon for the loot.
        self.credit(id, hit, "minecraft:player", true);
        if hit.died {
            self.kill_weapons.insert(id, attack.weapon.clone());
        }
        if hit.applied {
            self.villager_hurt_by(id, PLAYER_TARGET + player);
        }
        if hit.applied && hit.full {
            if let Some((body, _)) = self.mob_body(id) {
                let (xd, zd) = (attack.position.x - body.position.x, attack.position.z - body.position.z);
                self.knock_back(id, f64::from(0.4_f32), xd, zd);
            }
        }
        Some(hit)
    }
}
