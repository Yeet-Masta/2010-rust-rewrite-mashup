//! Player survival state. Movement exhaustion follows 26.3 ServerPlayer;
//! food ticking follows FoodData and peaceful regeneration follows ServerPlayer.

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "peaceful" => Some(Self::Peaceful),
            "easy" => Some(Self::Easy),
            "normal" => Some(Self::Normal),
            "hard" => Some(Self::Hard),
            _ => None,
        }
    }
}

/// The damage type's 26.3 `DamageScaling` rule. The attacker condition is
/// evaluated by `DamageSource.scalesWithDifficulty` before `Player.hurtServer`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DamageScaling {
    Never,
    WhenCausedByLivingNonPlayer,
    Always,
}

/// The status effects, under the name the survival code has used for them.
pub use crate::effects::{EffectInstance, MobEffect as EffectKind};
use crate::effects::{ActiveEffects, Attribute};

/// `Attributes.MAX_HEALTH`'s base for a player.
pub const BASE_MAX_HEALTH: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoodData {
    pub level: u8,
    pub saturation: f32,
    pub exhaustion: f32,
    tick_timer: u32,
}

impl Default for FoodData {
    fn default() -> Self {
        Self {
            level: 20,
            saturation: 5.0,
            exhaustion: 0.0,
            tick_timer: 0,
        }
    }
}

impl FoodData {
    pub fn add_exhaustion(&mut self, amount: f32) {
        self.exhaustion = (self.exhaustion + amount).min(40.0);
    }

    pub fn eat(&mut self, nutrition: u8, saturation: f32) {
        self.level = self.level.saturating_add(nutrition).min(20);
        self.saturation = (self.saturation + saturation).clamp(0.0, self.level as f32);
    }

    pub fn tick(
        &mut self,
        health: &mut f32,
        max_health: f32,
        natural_regeneration: bool,
        difficulty: Difficulty,
    ) -> bool {
        let mut starvation_hit = false;
        if self.exhaustion > 4.0 {
            self.exhaustion -= 4.0;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else if difficulty != Difficulty::Peaceful {
                self.level = self.level.saturating_sub(1);
            }
        }
        if natural_regeneration && self.saturation > 0.0 && *health < max_health && self.level >= 20
        {
            self.tick_timer += 1;
            if self.tick_timer >= 10 {
                let spent = self.saturation.min(6.0);
                *health = (*health + spent / 6.0).min(max_health);
                self.add_exhaustion(spent);
                self.tick_timer = 0;
            }
        } else if natural_regeneration && self.level >= 18 && *health < max_health {
            self.tick_timer += 1;
            if self.tick_timer >= 80 {
                *health = (*health + 1.0).min(max_health);
                self.add_exhaustion(6.0);
                self.tick_timer = 0;
            }
        } else if self.level == 0 {
            self.tick_timer += 1;
            if self.tick_timer >= 80 {
                let can_starve = match difficulty {
                    Difficulty::Peaceful | Difficulty::Easy => *health > 10.0,
                    Difficulty::Normal => *health > 1.0,
                    Difficulty::Hard => *health > 0.0,
                };
                if can_starve {
                    starvation_hit = true;
                }
                self.tick_timer = 0;
            }
        } else {
            self.tick_timer = 0;
        }
        starvation_hit
    }
}

/// The armor a hit meets: the `ARMOR` attribute's value and
/// `ARMOR_TOUGHNESS`, or none for damage that bypasses armor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Armor {
    pub value: Option<(f32, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SurvivalStatus {
    pub health: f32,
    pub max_health: f32,
    pub food: FoodData,
    pub experience_level: u32,
    pub experience_progress: f32,
    pub total_experience: u32,
    pub armor: u8,
    pub air: i32,
    pub max_air: i32,
    pub absorption: f32,
    /// `MAX_ABSORPTION`'s value: what absorption can hold (the absorption
    /// effect's 4 a level).
    pub max_absorption: f32,
    /// `remainingFireTicks`: burning while above zero; below, the ticks a
    /// fire block's touch takes to light the player (`getFireImmuneTicks`).
    pub fire_ticks: i32,
    /// Fire or lava hurt the player this tick, for the hurt sound
    /// (`Player.getHurtSound`'s `#is_fire`).
    pub burned: bool,
    damage_cooldown: u8,
    last_hurt: f32,
    /// `activeEffects`.
    pub effects: ActiveEffects,
    /// `tickCount`, which an infinite effect's intervals count.
    age: i32,
}

impl Default for SurvivalStatus {
    fn default() -> Self {
        Self {
            health: 20.0,
            max_health: 20.0,
            food: FoodData::default(),
            experience_level: 0,
            experience_progress: 0.0,
            total_experience: 0,
            armor: 0,
            air: 300,
            max_air: 300,
            absorption: 0.0,
            max_absorption: 0.0,
            fire_ticks: 0,
            burned: false,
            damage_cooldown: 0,
            last_hurt: 0.0,
            effects: ActiveEffects::default(),
            age: 0,
        }
    }
}

impl SurvivalStatus {
    pub fn damage_with_difficulty(
        &mut self,
        amount: f32,
        difficulty: Difficulty,
        scaling: DamageScaling,
        caused_by_living_non_player: bool,
    ) {
        let scales = match scaling {
            DamageScaling::Never => false,
            DamageScaling::WhenCausedByLivingNonPlayer => caused_by_living_non_player,
            DamageScaling::Always => true,
        };
        let amount = if scales {
            match difficulty {
                Difficulty::Peaceful => 0.0,
                Difficulty::Easy => (amount / 2.0 + 1.0).min(amount),
                Difficulty::Normal => amount,
                Difficulty::Hard => amount * 3.0 / 2.0,
            }
        } else {
            amount
        };
        if amount > 0.0 {
            self.damage(amount);
        }
    }

    /// `damageCooldownTime`: ticks left of the last hit's cooldown.
    pub fn damage_cooldown(&self) -> u8 {
        self.damage_cooldown
    }

    /// `getEffect`.
    pub fn effect(&self, kind: EffectKind) -> Option<&EffectInstance> {
        self.effects.get(kind)
    }

    /// `hasEffect`.
    pub fn has_effect(&self, kind: EffectKind) -> bool {
        self.effects.has(kind)
    }

    /// The attack damage attribute's `ADD_VALUE` modifiers from effects:
    /// strength's +3 and weakness's -4 a level.
    pub fn attack_damage_modifier(&self) -> f64 {
        self.effects.attribute(Attribute::AttackDamage, 0.0, &[])
    }

    /// `LivingEntity.heal`: only the living heal, up to their maximum.
    pub fn heal(&mut self, amount: f32) {
        if self.health > 0.0 {
            self.health = (self.health + amount).clamp(0.0, self.max_health);
        }
    }

    /// `setAbsorptionAmount`: held between none and `MAX_ABSORPTION`.
    pub fn set_absorption(&mut self, amount: f32) {
        self.absorption = amount.clamp(0.0, self.max_absorption);
    }

    /// `addEffect` of an effect at a level for a time, with particles and
    /// an icon.
    pub fn add_effect(&mut self, kind: EffectKind, duration: u32, amplifier: u8) {
        self.add_effect_instance(EffectInstance::new(kind, duration as i32, i32::from(amplifier)));
    }

    /// `LivingEntity.addEffect` (a player can be given every effect): a new
    /// effect, or an update of the one it has (`MobEffectInstance.update`),
    /// with its attributes refreshed (`onEffectAdded`, `onEffectUpdated`),
    /// then the given instance's `onEffectStarted` (absorption's hearts)
    /// whether or not it changed anything. Returns whether it did.
    pub fn add_effect_instance(&mut self, instance: EffectInstance) -> bool {
        let (kind, amplifier) = (instance.effect, instance.amplifier);
        let changed = self.effects.add(instance) != crate::effects::Added::Unchanged;
        if changed {
            self.refresh_attributes();
        }
        if kind == EffectKind::Absorption {
            self.set_absorption(self.absorption.max(4.0 * (1 + amplifier) as f32));
        }
        changed
    }

    /// `removeEffect`: whether it had it.
    pub fn remove_effect(&mut self, kind: EffectKind) -> bool {
        let removed = self.effects.remove(kind).is_some();
        if removed {
            self.refresh_attributes();
        }
        removed
    }

    /// `removeAllEffects` (milk): whether there were any.
    pub fn remove_all_effects(&mut self) -> bool {
        let any = self.effects.clear();
        if any {
            self.refresh_attributes();
        }
        any
    }

    /// `onAttributeUpdated` after the effects' modifiers change: health
    /// and absorption are cut to their new maxima.
    pub fn refresh_attributes(&mut self) {
        self.max_health = self.effects.attribute(Attribute::MaxHealth, f64::from(BASE_MAX_HEALTH), &[]).clamp(1.0, 1024.0) as f32;
        if self.health > self.max_health {
            self.health = self.max_health;
        }
        self.max_absorption = self.effects.attribute(Attribute::MaxAbsorption, 0.0, &[]).clamp(0.0, 2048.0) as f32;
        if self.absorption > self.max_absorption {
            self.absorption = self.max_absorption;
        }
    }

    /// `hurtServer` for damage that skips armor (`#bypasses_armor`, as
    /// magic, wither and fire do) and costs no food: the cooldown,
    /// resistance and absorption still apply.
    pub fn hurt_bypassing_armor(&mut self, amount: f32) {
        self.hurt(amount, Armor::default(), 0.0);
    }

    pub fn damage(&mut self, amount: f32) {
        self.damage_internal(amount, false);
    }

    /// `LivingEntity.hurtServer`'s damage cooldown around
    /// `Player.actuallyHurt`: within the cooldown's second half only the
    /// part above the last hit lands. `None` when the cooldown turns the
    /// hit away; otherwise whether it landed in full (knockback and the
    /// hurt animation follow a full hit).
    pub fn hurt(&mut self, damage: f32, armor: Armor, exhaustion: f32) -> Option<bool> {
        if self.health <= 0.0 {
            return None;
        }
        let damage = damage.max(0.0);
        if f32::from(self.damage_cooldown) > 10.0 {
            if damage <= self.last_hurt {
                return None;
            }
            self.actually_hurt(damage - self.last_hurt, armor, exhaustion);
            self.last_hurt = damage;
            Some(false)
        } else {
            self.last_hurt = damage;
            self.damage_cooldown = 20;
            self.actually_hurt(damage, armor, exhaustion);
            Some(true)
        }
    }

    /// `Player.actuallyHurt`: armor (`CombatRules.getDamageAfterAbsorb`),
    /// resistance, absorption, then health and the damage type's food
    /// exhaustion.
    fn actually_hurt(&mut self, mut damage: f32, armor: Armor, exhaustion: f32) {
        if let Some(armor) = armor.value {
            let toughness = 2.0 + armor.1 / 4.0;
            let real = (armor.0 - damage / toughness).clamp(armor.0 * 0.2, 20.0);
            damage *= 1.0 - real / 25.0;
        }
        if let Some(effect) = self.effect(EffectKind::Resistance) {
            let absorb = 25 - (effect.amplifier + 1) * 5;
            damage = (damage * absorb as f32 / 25.0).max(0.0);
        }
        if damage <= 0.0 {
            damage = 0.0;
        }
        let original = damage;
        damage = (damage - self.absorption).max(0.0);
        self.absorption = (self.absorption - (original - damage)).max(0.0);
        if damage != 0.0 {
            self.food.add_exhaustion(exhaustion);
            self.health = (self.health - damage).clamp(0.0, self.max_health);
            self.absorption = (self.absorption - damage).max(0.0);
        }
    }

    fn damage_internal(&mut self, amount: f32, bypass_effects: bool) {
        if self.health <= 0.0 {
            return;
        }
        let amount = if self.damage_cooldown > 10 {
            if amount <= self.last_hurt {
                return;
            }
            let difference = amount - self.last_hurt;
            self.last_hurt = amount;
            difference
        } else {
            self.last_hurt = amount;
            self.damage_cooldown = 20;
            amount
        };
        let mut remaining = amount;
        if let Some(effect) = self.effect(EffectKind::Resistance).filter(|_| !bypass_effects) {
            let absorb = 25 - (effect.amplifier + 1) * 5;
            remaining = (remaining * absorb as f32 / 25.0).max(0.0);
        }
        let absorbed = remaining.min(self.absorption);
        self.absorption -= absorbed;
        remaining -= absorbed;
        self.health = (self.health - remaining).max(0.0);
    }

    /// `LivingEntity.tickEffects` for a vulnerable player.
    pub fn tick_effects(&mut self) {
        self.tick_effects_as(false);
    }

    /// `LivingEntity.tickEffects`, `MobEffectInstance.tickServer` and
    /// `tickClient`'s blend: each effect works if its interval falls on
    /// this tick (an infinite one counts the entity's age), then counts
    /// down, coming back to a hidden weaker effect when spent; it ends when
    /// its time runs out or its work says so (absorption with its hearts
    /// gone). An invulnerable (creative) player takes no harm from them.
    pub fn tick_effects_as(&mut self, invulnerable: bool) {
        self.damage_cooldown = self.damage_cooldown.saturating_sub(1);
        self.age = self.age.wrapping_add(1);
        let kinds: Vec<EffectKind> = self.effects.iter().map(|e| e.effect).collect();
        let mut changed = false;
        for kind in kinds {
            let Some(effect) = self.effects.get(kind) else { continue };
            let mut keep = effect.has_remaining_duration();
            if keep {
                let ticks = if effect.infinite() { self.age } else { effect.duration };
                let amplifier = effect.amplifier;
                if kind.applies_this_tick(ticks, amplifier) {
                    keep = self.apply_effect_tick(kind, amplifier, invulnerable);
                }
            }
            if let Some(effect) = self.effects.get_mut(kind).filter(|_| keep) {
                effect.tick_down();
                changed |= effect.downgrade_to_hidden();
                keep = effect.has_remaining_duration();
            }
            if !keep {
                self.effects.remove(kind);
                changed = true;
            }
        }
        if changed {
            self.refresh_attributes();
        }
        for effect in self.effects.iter_mut() {
            effect.tick_blend();
        }
    }

    /// `MobEffect.applyEffectTick` for the effects with work to do on
    /// their ticks; false ends the effect.
    fn apply_effect_tick(&mut self, kind: EffectKind, amplifier: i32, invulnerable: bool) -> bool {
        // Java's shifts take their distance modulo 32.
        let shifted = |base: i32| base.wrapping_shl(amplifier as u32);
        match kind {
            EffectKind::Regeneration if self.health < self.max_health => self.heal(1.0),
            EffectKind::Poison if self.health > 1.0 && !invulnerable => self.hurt_bypassing_armor(1.0),
            EffectKind::Wither if !invulnerable => self.hurt_bypassing_armor(1.0),
            EffectKind::Hunger => self.food.add_exhaustion(0.005 * (amplifier + 1) as f32),
            // `FoodData.eat(amplifier + 1, 1.0)`: saturation by modifier.
            EffectKind::Saturation => {
                let nutrition = (amplifier + 1).clamp(0, 255) as u8;
                self.food.eat(nutrition, f32::from(nutrition) * 1.0 * 2.0);
            }
            EffectKind::InstantHealth => self.heal(shifted(4).max(0) as f32),
            EffectKind::InstantDamage if !invulnerable => self.hurt_bypassing_armor(shifted(6) as f32),
            EffectKind::Absorption => return self.absorption > 0.0,
            _ => {}
        }
        true
    }

    /// `HealOrHarmMobEffect.applyInstantaneousEffect` on the player at a
    /// splash's nearness: healing 4 or harm 6 doubling a level, scaled and
    /// rounded.
    pub fn apply_instant(&mut self, kind: EffectKind, amplifier: i32, scale: f64, invulnerable: bool) {
        let shifted = |base: i32| base.wrapping_shl(amplifier as u32);
        match kind {
            EffectKind::InstantHealth => self.heal((scale * f64::from(shifted(4)) + 0.5) as i32 as f32),
            EffectKind::InstantDamage if !invulnerable => {
                self.hurt_bypassing_armor((scale * f64::from(shifted(6)) + 0.5) as i32 as f32);
            }
            EffectKind::Saturation => {
                self.apply_effect_tick(kind, amplifier, invulnerable);
            }
            _ => {}
        }
    }

    /// `isOnFire`.
    pub fn on_fire(&self) -> bool {
        self.fire_ticks > 0
    }

    /// `Entity.baseTick`'s burning: a point of `on_fire` damage (through
    /// no armor) each second, none while in lava, and a tick off the fire.
    pub fn tick_fire(&mut self, in_lava: bool, invulnerable: bool) {
        self.burned = false;
        if self.fire_ticks > 0 {
            if self.fire_ticks % 20 == 0 && !in_lava {
                self.hurt_by_fire(1.0, Armor::default(), 0.0, invulnerable);
            }
            self.fire_ticks -= 1;
        }
    }

    /// `hurtServer` for `#is_fire` damage, which fire resistance turns away.
    pub fn hurt_by_fire(&mut self, amount: f32, armor: Armor, exhaustion: f32, invulnerable: bool) {
        if invulnerable || self.has_effect(EffectKind::FireResistance) {
            return;
        }
        if self.hurt(amount, armor, exhaustion).is_some() {
            self.burned = true;
        }
    }

    /// `igniteForTicks`: burning at least this long.
    pub fn ignite_for_ticks(&mut self, ticks: i32) {
        if self.fire_ticks < ticks {
            self.fire_ticks = ticks;
        }
    }

    /// `clearFire`: put out, keeping any immunity left.
    pub fn clear_fire(&mut self) {
        self.fire_ticks = self.fire_ticks.min(0);
    }

    /// `BaseFireBlock.fireIgnite` for a player in a fire block: the
    /// immunity counts down a tick, or the fire grows by `added` (the
    /// level random's 1 or 2); once not immune it burns for eight seconds.
    pub fn fire_ignite(&mut self, added: i32) {
        if self.fire_ticks < 0 {
            self.fire_ticks += 1;
        } else {
            self.fire_ticks += added;
        }
        if self.fire_ticks >= 0 {
            self.ignite_for_ticks(160);
        }
    }

    /// `Entity.thunderHit`'s fire: struck while not immune, eight seconds.
    pub fn thunder_ignite(&mut self) {
        self.fire_ticks += 1;
        if self.fire_ticks == 0 {
            self.ignite_for_ticks(160);
        }
    }

    /// `lavaIgnite` and `lavaHurt`: fifteen seconds alight and 4 damage
    /// (`lava`, through armor). Whether it hurt.
    pub fn touch_lava(&mut self) -> bool {
        self.touch_lava_with(Armor::default(), false)
    }

    pub fn touch_lava_with(&mut self, armor: Armor, invulnerable: bool) -> bool {
        self.ignite_for_ticks(300);
        let health = self.health;
        self.hurt_by_fire(4.0, armor, 0.1, invulnerable);
        self.health < health
    }

    /// LivingEntity.baseTick consumes one air per submerged tick. Without
    /// oxygen-bonus or water-breathing effects (`hasWaterBreathing`),
    /// damage lands at -20 air and resets air to zero; surfacing restores
    /// four air per tick, as breathing under water does unless the
    /// nautilus's breath alone holds it (`shouldEffectsRefillAirsupply`).
    pub fn tick_air(&mut self, eye_submerged: bool, invulnerable: bool) -> bool {
        let can_drown = !invulnerable && !self.effects.water_breathing();
        if eye_submerged && can_drown {
            self.air -= 1;
            if self.air <= -20 {
                self.air = 0;
                self.damage(2.0);
                return true;
            }
        } else if self.air < self.max_air && (!eye_submerged || self.effects.refill_air()) {
            self.air = (self.air + 4).min(self.max_air);
        }
        false
    }

    pub fn tick_normal(&mut self) {
        self.tick_with_rules(true, Difficulty::Normal, 0);
    }

    pub fn tick_normal_with_regen(&mut self, natural_regeneration: bool) {
        self.tick_with_rules(natural_regeneration, Difficulty::Normal, 0);
    }

    pub fn tick_with_rules(
        &mut self,
        natural_regeneration: bool,
        difficulty: Difficulty,
        player_tick: u64,
    ) {
        if natural_regeneration && difficulty == Difficulty::Peaceful {
            if player_tick % 20 == 0 {
                self.health = (self.health + 1.0).min(self.max_health);
                self.food.saturation = (self.food.saturation + 1.0).min(20.0);
            }
            if player_tick % 10 == 0 {
                self.food.level = self.food.level.saturating_add(1).min(20);
            }
        }
        let starvation_hit = self.food.tick(
            &mut self.health,
            self.max_health,
            natural_regeneration,
            difficulty,
        );
        if starvation_hit {
            // The default starve damage type bypasses armor and effects, but
            // still enters LivingEntity's hurt cooldown and absorption path.
            self.damage_internal(1.0, true);
        }
    }

    pub fn experience_to_next_level(&self) -> u32 {
        match self.experience_level {
            0..=14 => 7 + self.experience_level * 2,
            15..=29 => 37 + (self.experience_level - 15) * 5,
            level => 112 + (level - 30) * 9,
        }
    }

    pub fn give_experience_points(&mut self, points: u32) {
        self.experience_progress += points as f32 / self.experience_to_next_level() as f32;
        self.total_experience = self.total_experience.saturating_add(points);
        while self.experience_progress >= 1.0 {
            self.experience_progress =
                (self.experience_progress - 1.0) * self.experience_to_next_level() as f32;
            self.experience_level = self.experience_level.saturating_add(1);
            self.experience_progress /= self.experience_to_next_level() as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhaustion_spends_saturation_then_food_one_point_per_tick() {
        let mut food = FoodData::default();
        let mut health = 20.0;
        food.add_exhaustion(8.1);
        food.tick(&mut health, 20.0, true, Difficulty::Normal);
        assert_eq!((food.saturation, food.level), (4.0, 20));
        food.tick(&mut health, 20.0, true, Difficulty::Normal);
        assert_eq!((food.saturation, food.level), (3.0, 20));
        food.saturation = 0.0;
        food.add_exhaustion(4.0);
        food.tick(&mut health, 20.0, true, Difficulty::Normal);
        assert_eq!(food.level, 19);
    }

    #[test]
    fn experience_crosses_vanilla_level_thresholds() {
        let mut status = SurvivalStatus::default();
        status.give_experience_points(7);
        assert_eq!(status.experience_level, 1);
        assert_eq!(status.experience_progress, 0.0);
        status.give_experience_points(4);
        assert_eq!(status.experience_level, 1);
        assert!((status.experience_progress - 4.0 / 9.0).abs() < 1e-6);
    }

    #[test]
    fn underwater_air_damages_every_twenty_ticks_and_recovers_by_four() {
        let mut status = SurvivalStatus::default();
        for _ in 0..319 {
            status.tick_effects();
            assert!(!status.tick_air(true, false));
        }
        assert_eq!(status.air, -19);
        assert_eq!(status.health, 20.0);
        status.tick_effects();
        assert!(status.tick_air(true, false));
        assert_eq!((status.air, status.health), (0, 18.0));
        for _ in 0..19 {
            status.tick_effects();
            assert!(!status.tick_air(true, false));
        }
        status.tick_effects();
        assert!(status.tick_air(true, false));
        assert_eq!(status.health, 16.0);
        assert!(!status.tick_air(false, false));
        assert_eq!(status.air, 4);
    }

    #[test]
    fn enclosed_water_client_trace_repeats_the_air_cycle() {
        // Two isolated 26.3 client runs of scenarios/survival/drown.json had
        // identical air at every one of 360 recorded ticks. The setup command
        // had already consumed 20 air before tick zero.
        let mut status = SurvivalStatus::default();
        status.air = 280;
        for tick in 0..360 {
            status.tick_air(true, false);
            if let Some(expected) = match tick {
                0 => Some(279),
                279 | 299 | 319 | 339 | 359 => Some(0),
                280 => Some(-1),
                298 | 318 | 338 | 358 => Some(-19),
                _ => None,
            } {
                assert_eq!(status.air, expected, "client tick {tick}");
            }
        }
    }

    #[test]
    fn disabled_natural_regeneration_allows_drowning_to_kill() {
        let mut status = SurvivalStatus::default();
        status.air = 280;
        let mut fatal_tick = None;
        for tick in 0..500 {
            status.tick_effects();
            status.tick_air(true, false);
            status.tick_normal_with_regen(false);
            if status.health == 0.0 {
                fatal_tick = Some(tick);
                break;
            }
        }
        assert_eq!(fatal_tick, Some(479));
        assert_eq!(status.food.level, 20);
        assert_eq!(status.food.saturation, 5.0);
    }

    #[test]
    fn peaceful_refills_food_and_heals_while_normal_regeneration_spends_hunger() {
        let mut peaceful = SurvivalStatus::default();
        peaceful.health = 2.0;
        peaceful.food.saturation = 0.0;
        peaceful.food.exhaustion = 40.0;
        for tick in 1..=500 {
            peaceful.tick_with_rules(true, Difficulty::Peaceful, tick);
        }
        assert_eq!(peaceful.food.level, 20);
        assert_eq!(peaceful.health, 20.0);

        let mut normal = SurvivalStatus::default();
        normal.health = 2.0;
        normal.food.saturation = 0.0;
        for tick in 1..=500 {
            normal.tick_with_rules(true, Difficulty::Normal, tick);
        }
        assert!(normal.food.level < 18);
        assert!(normal.health < 20.0);
        let stopped_at = normal.health;
        for tick in 501..=650 {
            normal.tick_with_rules(true, Difficulty::Normal, tick);
        }
        assert_eq!(normal.health, stopped_at);
    }

    #[test]
    fn starvation_respects_easy_normal_and_hard_health_floors() {
        for (difficulty, expected) in [
            (Difficulty::Easy, 10.0),
            (Difficulty::Normal, 1.0),
            (Difficulty::Hard, 0.0),
        ] {
            let mut status = SurvivalStatus::default();
            status.health = 12.0;
            status.food.level = 0;
            status.food.saturation = 0.0;
            for tick in 1..=1000 {
                status.tick_effects();
                status.tick_with_rules(false, difficulty, tick);
            }
            assert_eq!(status.health, expected, "{difficulty:?}");
        }
    }

    #[test]
    fn starvation_uses_hurt_cooldown_and_bypasses_resistance_before_absorption() {
        let mut status = SurvivalStatus::default();
        status.health = 12.0;
        status.food.level = 0;
        status.food.saturation = 0.0;
        status.add_effect(EffectKind::Resistance, 200, 4);
        status.add_effect(EffectKind::Absorption, 200, 0);
        for tick in 1..=80 {
            status.tick_effects();
            status.tick_with_rules(false, Difficulty::Hard, tick);
        }
        assert_eq!(status.health, 12.0);
        assert_eq!(status.absorption, 3.0);

        let mut recently_hurt = SurvivalStatus::default();
        recently_hurt.health = 12.0;
        recently_hurt.food.level = 0;
        recently_hurt.food.saturation = 0.0;
        for tick in 1..=80 {
            recently_hurt.tick_effects();
            if tick == 75 {
                recently_hurt.damage(2.0);
            }
            recently_hurt.tick_with_rules(false, Difficulty::Hard, tick);
        }
        assert_eq!(recently_hurt.health, 10.0);
    }

    #[test]
    fn all_difficulties_match_repeated_26_3_server_food_traces_every_tick() {
        // Each fixture row was observed at END_SERVER_TICK after an explicit
        // START_SERVER_TICK setup. The exporter refuses non-identical vanilla
        // repeats before this CSV can be regenerated.
        // The fixture lives in MinecraftOSS's scenarios, which this copy
        // does not vendor; without it there is nothing to compare.
        let Ok(reference) = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/survival/difficulty-reference.csv")) else {
            return;
        };
        let mut current_profile = "";
        let mut status = SurvivalStatus::default();
        for row in reference.lines().skip(1) {
            let fields: Vec<_> = row.split(',').collect();
            assert_eq!(fields.len(), 7);
            let profile = fields[0];
            if profile != current_profile {
                current_profile = profile;
                status = SurvivalStatus::default();
                status.health = if profile.ends_with("_starve") {
                    2.0
                } else {
                    12.0
                };
                status.food.level = if profile.ends_with("_starve") { 0 } else { 20 };
                status.food.saturation = 0.0;
            }
            let tick: u64 = fields[1].parse().unwrap();
            let player_tick: u64 = fields[2].parse().unwrap();
            assert_eq!(player_tick, tick + 1, "{profile} tick {tick}");
            let difficulty = Difficulty::parse(profile.split('_').next().unwrap()).unwrap();
            status.tick_effects();
            status.tick_with_rules(true, difficulty, player_tick);
            assert_eq!(
                (status.health as f64).to_bits(),
                u64::from_str_radix(fields[3], 16).unwrap(),
                "{profile} tick {tick}: health"
            );
            assert_eq!(
                status.food.level,
                fields[4].parse::<u8>().unwrap(),
                "{profile} tick {tick}: food"
            );
            assert_eq!(
                (status.food.saturation as f64).to_bits(),
                u64::from_str_radix(fields[5], 16).unwrap(),
                "{profile} tick {tick}: saturation"
            );
            assert_eq!(
                status.health <= 0.0,
                fields[6] == "1",
                "{profile} tick {tick}: death"
            );
        }
    }

    #[test]
    fn difficulty_scales_mob_damage_but_not_fall_damage() {
        // 26.3 integrated-server probes: six base damage, player at twenty
        // health, hit after the initial join protection expires.
        for (difficulty, mob_health) in [
            (Difficulty::Peaceful, 20.0),
            (Difficulty::Easy, 16.0),
            (Difficulty::Normal, 14.0),
            (Difficulty::Hard, 11.0),
        ] {
            let mut mob = SurvivalStatus::default();
            mob.damage_with_difficulty(
                6.0,
                difficulty,
                DamageScaling::WhenCausedByLivingNonPlayer,
                true,
            );
            assert_eq!(mob.health, mob_health, "{difficulty:?} mob attack");
            if difficulty == Difficulty::Peaceful {
                // Zero damage must not start the ordinary hurt cooldown.
                mob.damage_with_difficulty(
                    6.0,
                    difficulty,
                    DamageScaling::WhenCausedByLivingNonPlayer,
                    false,
                );
                assert_eq!(mob.health, 14.0);
            }
            let mut fall = SurvivalStatus::default();
            fall.damage_with_difficulty(
                6.0,
                difficulty,
                DamageScaling::WhenCausedByLivingNonPlayer,
                false,
            );
            assert_eq!(fall.health, 14.0, "{difficulty:?} fall");
        }
        let mut player_attacker = SurvivalStatus::default();
        player_attacker.damage_with_difficulty(
            6.0,
            Difficulty::Hard,
            DamageScaling::WhenCausedByLivingNonPlayer,
            false,
        );
        assert_eq!(player_attacker.health, 14.0);
    }

    #[test]
    fn all_difficulties_match_repeated_26_3_server_damage_traces() {
        // The fixture lives in MinecraftOSS's scenarios, which this copy
        // does not vendor; without it there is nothing to compare.
        let Ok(reference) = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/survival/difficulty-damage-reference.csv")) else {
            return;
        };
        let mut current_profile = "";
        let mut status = SurvivalStatus::default();
        for row in reference.lines().skip(1) {
            let fields: Vec<_> = row.split(',').collect();
            assert_eq!(fields.len(), 7);
            let profile = fields[0];
            let difficulty = Difficulty::parse(profile.split('_').next().unwrap()).unwrap();
            if profile != current_profile {
                current_profile = profile;
                status = SurvivalStatus::default();
                let mob = profile.ends_with("_mob_damage");
                status.damage_with_difficulty(
                    6.0,
                    difficulty,
                    DamageScaling::WhenCausedByLivingNonPlayer,
                    mob,
                );
            }
            let tick: u64 = fields[1].parse().unwrap();
            let player_tick: u64 = fields[2].parse().unwrap();
            status.tick_effects();
            status.tick_with_rules(true, difficulty, player_tick);
            assert_eq!(
                (status.health as f64).to_bits(),
                u64::from_str_radix(fields[3], 16).unwrap(),
                "{profile} tick {tick}: health"
            );
            assert_eq!(
                status.food.level,
                fields[4].parse::<u8>().unwrap(),
                "{profile} tick {tick}: food"
            );
            assert_eq!(
                (status.food.saturation as f64).to_bits(),
                u64::from_str_radix(fields[5], 16).unwrap(),
                "{profile} tick {tick}: saturation"
            );
            assert_eq!(
                status.health <= 0.0,
                fields[6] == "1",
                "{profile} tick {tick}: death"
            );
        }
    }

    #[test]
    fn lava_damage_obeys_living_entity_cooldown_and_water_extinguishes_fire() {
        let mut status = SurvivalStatus::default();
        status.touch_lava();
        assert_eq!((status.health, status.fire_ticks), (16.0, 300));
        for _ in 0..9 {
            status.tick_effects();
            status.tick_fire(true, false);
            status.touch_lava();
        }
        assert_eq!(status.health, 16.0);
        status.tick_effects();
        status.tick_fire(true, false);
        status.touch_lava();
        assert_eq!(status.health, 12.0);
        status.clear_fire();
        assert_eq!(status.fire_ticks, 0);
        status.add_effect(EffectKind::FireResistance, 100, 0);
        status.touch_lava();
        assert_eq!(status.health, 12.0);
    }

    #[test]
    fn fire_blocks_wait_out_the_immunity_then_burn_eight_seconds() {
        let mut status = SurvivalStatus::default();
        status.fire_ticks = -20;
        for _ in 0..19 {
            status.fire_ignite(1);
        }
        assert_eq!(status.fire_ticks, -1);
        status.fire_ignite(2);
        assert_eq!(status.fire_ticks, 160);
        // A point each second, not in lava.
        let health = status.health;
        status.fire_ticks = 20;
        status.tick_fire(false, false);
        assert_eq!((status.health, status.fire_ticks, status.burned), (health - 1.0, 19, true));
        status.thunder_ignite();
        assert_eq!(status.fire_ticks, 20);
    }

    #[test]
    fn measured_food_effects_tick_at_remaining_duration_intervals() {
        let mut status = SurvivalStatus::default();
        status.health = 10.0;
        status.add_effect(EffectKind::Regeneration, 100, 1);
        status.add_effect(EffectKind::Hunger, 2, 2);
        status.tick_effects();
        assert_eq!(status.health, 11.0);
        assert!((status.food.exhaustion - 0.015).abs() < 1e-6);
        assert_eq!(
            status.effect(EffectKind::Regeneration).unwrap().duration,
            99
        );
        status.tick_effects();
        assert_eq!(status.effect(EffectKind::Hunger), None);
        for _ in 0..24 {
            status.tick_effects();
        }
        assert_eq!(status.health, 12.0);
    }

    #[test]
    fn poison_stops_at_one_health_point_and_absorption_precedes_resisted_damage() {
        let mut status = SurvivalStatus::default();
        status.health = 2.0;
        status.add_effect(EffectKind::Poison, 25, 0);
        status.tick_effects();
        assert_eq!(status.health, 1.0);
        for _ in 0..24 {
            status.tick_effects();
        }
        assert_eq!(status.health, 1.0);
        status.add_effect(EffectKind::Resistance, 100, 0);
        status.add_effect(EffectKind::Absorption, 100, 0);
        status.damage(5.0);
        assert_eq!((status.absorption, status.health), (0.0, 1.0));
    }
}
