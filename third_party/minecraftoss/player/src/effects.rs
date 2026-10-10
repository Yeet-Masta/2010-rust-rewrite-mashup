//! Status effects (pinned 26.3 `MobEffect`, `MobEffects`, `MobEffectCategory`,
//! `MobEffectInstance`, `MobEffectUtil`, `Potion`, `Potions` and
//! `PotionContents`). Every registered effect is here with its category,
//! colour and attribute modifiers; an instance keeps a weaker but longer
//! effect hidden under a stronger one and falls back to it when its own
//! time runs out, and the client fades the screen effects in and out with
//! its blend state. What an effect does on its ticks is the living
//! entity's business (`SurvivalStatus::tick_effects`).
//!
//! Vanilla keeps active effects in a hash map keyed by identity-hashed
//! holders, so the order two effects work in on the same tick varies from
//! run to run; here they work in the order they were added.

use serde_json::{json, Value};

/// `MobEffectCategory`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Beneficial,
    Harmful,
    Neutral,
}

/// An attribute an effect modifies (`Attributes`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attribute {
    MovementSpeed,
    AttackSpeed,
    AttackDamage,
    SafeFallDistance,
    MaxHealth,
    MaxAbsorption,
    Luck,
    WaypointTransmitRange,
}

/// `AttributeModifier.Operation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    AddValue,
    AddMultipliedTotal,
}

/// A registered status effect (`MobEffects`, in registry order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MobEffect {
    Speed,
    Slowness,
    Haste,
    MiningFatigue,
    Strength,
    InstantHealth,
    InstantDamage,
    JumpBoost,
    Nausea,
    Regeneration,
    Resistance,
    FireResistance,
    WaterBreathing,
    Invisibility,
    Blindness,
    NightVision,
    Hunger,
    Weakness,
    Poison,
    Wither,
    HealthBoost,
    Absorption,
    Saturation,
    Glowing,
    Levitation,
    Luck,
    Unluck,
    SlowFalling,
    ConduitPower,
    DolphinsGrace,
    BadOmen,
    HeroOfTheVillage,
    Darkness,
    TrialOmen,
    RaidOmen,
    WindCharged,
    Weaving,
    Oozing,
    Infested,
    BreathOfTheNautilus,
}

use MobEffect::*;

/// Each effect's registry name, category and colour, in registry order.
const REGISTRY: [(MobEffect, &str, Category, u32); 40] = [
    (Speed, "speed", Category::Beneficial, 3402751),
    (Slowness, "slowness", Category::Harmful, 9154528),
    (Haste, "haste", Category::Beneficial, 14270531),
    (MiningFatigue, "mining_fatigue", Category::Harmful, 4866583),
    (Strength, "strength", Category::Beneficial, 16762624),
    (InstantHealth, "instant_health", Category::Beneficial, 16262179),
    (InstantDamage, "instant_damage", Category::Harmful, 11101546),
    (JumpBoost, "jump_boost", Category::Beneficial, 16646020),
    (Nausea, "nausea", Category::Harmful, 5578058),
    (Regeneration, "regeneration", Category::Beneficial, 13458603),
    (Resistance, "resistance", Category::Beneficial, 9520880),
    (FireResistance, "fire_resistance", Category::Beneficial, 16750848),
    (WaterBreathing, "water_breathing", Category::Beneficial, 10017472),
    (Invisibility, "invisibility", Category::Beneficial, 16185078),
    (Blindness, "blindness", Category::Harmful, 2039587),
    (NightVision, "night_vision", Category::Beneficial, 12779366),
    (Hunger, "hunger", Category::Harmful, 5797459),
    (Weakness, "weakness", Category::Harmful, 4738376),
    (Poison, "poison", Category::Harmful, 8889187),
    (Wither, "wither", Category::Harmful, 7561558),
    (HealthBoost, "health_boost", Category::Beneficial, 16284963),
    (Absorption, "absorption", Category::Beneficial, 2445989),
    (Saturation, "saturation", Category::Beneficial, 16262179),
    (Glowing, "glowing", Category::Neutral, 9740385),
    (Levitation, "levitation", Category::Harmful, 13565951),
    (Luck, "luck", Category::Beneficial, 5882118),
    (Unluck, "unluck", Category::Harmful, 12624973),
    (SlowFalling, "slow_falling", Category::Beneficial, 15978425),
    (ConduitPower, "conduit_power", Category::Beneficial, 1950417),
    (DolphinsGrace, "dolphins_grace", Category::Beneficial, 8954814),
    (BadOmen, "bad_omen", Category::Neutral, 745784),
    (HeroOfTheVillage, "hero_of_the_village", Category::Beneficial, 4521796),
    (Darkness, "darkness", Category::Harmful, 2696993),
    (TrialOmen, "trial_omen", Category::Neutral, 1484454),
    (RaidOmen, "raid_omen", Category::Neutral, 14565464),
    (WindCharged, "wind_charged", Category::Harmful, 12438015),
    (Weaving, "weaving", Category::Harmful, 7891290),
    (Oozing, "oozing", Category::Harmful, 10092451),
    (Infested, "infested", Category::Harmful, 9214860),
    (BreathOfTheNautilus, "breath_of_the_nautilus", Category::Beneficial, 65518),
];

impl MobEffect {
    /// Every effect, in registry order.
    pub fn all() -> impl Iterator<Item = MobEffect> {
        REGISTRY.iter().map(|entry| entry.0)
    }

    fn entry(self) -> &'static (MobEffect, &'static str, Category, u32) {
        &REGISTRY[self as usize]
    }

    /// Its registry name without the namespace (`speed`).
    pub fn name(self) -> &'static str {
        self.entry().1
    }

    /// Its registry ID (`minecraft:speed`).
    pub fn id(self) -> String {
        format!("minecraft:{}", self.name())
    }

    /// The effect a registry ID names, with or without its namespace.
    pub fn from_id(id: &str) -> Option<Self> {
        let name = id.strip_prefix("minecraft:").unwrap_or(id);
        REGISTRY.iter().find(|entry| entry.1 == name).map(|entry| entry.0)
    }

    /// Its translation key (`MobEffect.getDescriptionId`).
    pub fn description_id(self) -> String {
        format!("effect.minecraft.{}", self.name())
    }

    pub fn category(self) -> Category {
        self.entry().2
    }

    /// `isBeneficial`.
    pub fn beneficial(self) -> bool {
        self.category() == Category::Beneficial
    }

    /// Its colour, RGB.
    pub fn color(self) -> u32 {
        self.entry().3
    }

    /// `isInstantaneous`: the `InstantaneousMobEffect`s.
    pub fn instantaneous(self) -> bool {
        matches!(self, InstantHealth | InstantDamage | Saturation)
    }

    /// `getBlendInDurationTicks`, `getBlendOutDurationTicks` and
    /// `getBlendOutAdvanceTicks` (`setBlendDuration`).
    pub fn blend_durations(self) -> (i32, i32, i32) {
        match self {
            Nausea => (150, 20, 60),
            Darkness => (22, 22, 22),
            _ => (0, 0, 0),
        }
    }

    /// The sound played when it is first added (`withSoundOnAdded`).
    pub fn sound_on_added(self) -> Option<&'static str> {
        match self {
            BadOmen => Some("minecraft:event.mob_effect.bad_omen"),
            TrialOmen => Some("minecraft:event.mob_effect.trial_omen"),
            RaidOmen => Some("minecraft:event.mob_effect.raid_omen"),
            _ => None,
        }
    }

    /// Its attribute modifiers at level 1 (`addAttributeModifier`; float
    /// literals widened as vanilla's are). A level multiplies the amount
    /// (`AttributeTemplate.create`).
    pub fn modifiers(self) -> &'static [(Attribute, f32, Operation)] {
        use Attribute as A;
        use Operation::*;
        match self {
            Speed => &[(A::MovementSpeed, 0.2, AddMultipliedTotal)],
            Slowness => &[(A::MovementSpeed, -0.15, AddMultipliedTotal)],
            Haste => &[(A::AttackSpeed, 0.1, AddMultipliedTotal)],
            MiningFatigue => &[(A::AttackSpeed, -0.1, AddMultipliedTotal)],
            Strength => &[(A::AttackDamage, 3.0, AddValue)],
            JumpBoost => &[(A::SafeFallDistance, 1.0, AddValue)],
            Invisibility => &[(A::WaypointTransmitRange, -1.0, AddMultipliedTotal)],
            Weakness => &[(A::AttackDamage, -4.0, AddValue)],
            HealthBoost => &[(A::MaxHealth, 4.0, AddValue)],
            Absorption => &[(A::MaxAbsorption, 4.0, AddValue)],
            Luck => &[(A::Luck, 1.0, AddValue)],
            Unluck => &[(A::Luck, -1.0, AddValue)],
            _ => &[],
        }
    }

    /// `shouldApplyEffectTickThisTick` for the ticks left (or the entity's
    /// age, for an infinite effect): poison every 25 ticks, wither every
    /// 40 and regeneration every 50, halved a level (every tick once the
    /// shift runs out); hunger, absorption and bad omen every tick; raid
    /// omen on its last; an instant effect while any time is left.
    pub fn applies_this_tick(self, ticks: i32, amplifier: i32) -> bool {
        let every = |interval: i32| {
            // Java's shift takes its distance modulo 32.
            let interval = interval.wrapping_shr(amplifier as u32);
            interval <= 0 || ticks % interval == 0
        };
        match self {
            InstantHealth | InstantDamage | Saturation => ticks >= 1,
            Poison => every(25),
            Wither => every(40),
            Regeneration => every(50),
            Hunger | Absorption | BadOmen => true,
            RaidOmen => ticks == 1,
            _ => false,
        }
    }
}

/// An effect's fade in and out for the screen effects
/// (`MobEffectInstance.BlendState`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Blend {
    pub factor: f32,
    pub previous: f32,
}

/// `MobEffectInstance`: an effect's time left (-1 forever), level and
/// display flags, with the weaker effect hidden under it.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectInstance {
    pub effect: MobEffect,
    pub duration: i32,
    pub amplifier: i32,
    pub ambient: bool,
    /// `visible`: whether it shows particles.
    pub visible: bool,
    pub show_icon: bool,
    pub hidden: Option<Box<EffectInstance>>,
    pub blend: Blend,
}

/// `MobEffectInstance.INFINITE_DURATION`.
pub const INFINITE: i32 = -1;

impl EffectInstance {
    /// `new MobEffectInstance(effect, duration, amplifier)`: not ambient,
    /// with particles and an icon.
    pub fn new(effect: MobEffect, duration: i32, amplifier: i32) -> Self {
        Self::with_flags(effect, duration, amplifier, false, true, true)
    }

    /// `new MobEffectInstance(effect, duration, amplifier, ambient,
    /// visible, showIcon)`.
    pub fn with_flags(effect: MobEffect, duration: i32, amplifier: i32, ambient: bool, visible: bool, show_icon: bool) -> Self {
        Self { effect, duration, amplifier: amplifier.clamp(0, 255), ambient, visible, show_icon, hidden: None, blend: Blend::default() }
    }

    /// `MobEffectInstance(copy)`: the details without the hidden effect.
    pub fn copy(&self) -> Self {
        Self { hidden: None, blend: Blend::default(), ..self.clone() }
    }

    /// `isInfiniteDuration`.
    pub fn infinite(&self) -> bool {
        self.duration == INFINITE
    }

    /// `hasRemainingDuration`.
    pub fn has_remaining_duration(&self) -> bool {
        self.infinite() || self.duration > 0
    }

    fn shorter_than(&self, other: &Self) -> bool {
        !self.infinite() && (self.duration < other.duration || other.infinite())
    }

    /// `endsWithin`.
    pub fn ends_within(&self, ticks: i32) -> bool {
        !self.infinite() && self.duration <= ticks
    }

    /// `mapDuration`: infinite and spent durations stay as they are.
    pub fn map_duration(&self, f: impl FnOnce(i32) -> i32) -> i32 {
        if !self.infinite() && self.duration != 0 {
            f(self.duration)
        } else {
            self.duration
        }
    }

    /// `withScaledDuration`: a copy lasting `scale` as long, at least a
    /// tick.
    pub fn with_scaled_duration(&self, scale: f32) -> Self {
        let mut copy = self.copy();
        copy.duration = self.map_duration(|d| ((d as f32 * scale).floor() as i32).max(1));
        copy
    }

    /// `update(takeOver)`: a stronger effect takes over (hiding this one if
    /// it would outlast it), a longer one of the same level extends it, and
    /// a longer weaker one hides beneath. Returns whether it changed.
    pub fn update(&mut self, take_over: &Self) -> bool {
        let mut changed = false;
        if take_over.amplifier > self.amplifier {
            if take_over.shorter_than(self) {
                let previous = self.hidden.take();
                let mut hidden = self.copy();
                hidden.hidden = previous;
                self.hidden = Some(Box::new(hidden));
            }
            self.amplifier = take_over.amplifier;
            self.duration = take_over.duration;
            changed = true;
        } else if self.shorter_than(take_over) {
            if take_over.amplifier == self.amplifier {
                self.duration = take_over.duration;
                changed = true;
            } else if let Some(hidden) = self.hidden.as_mut() {
                hidden.update(take_over);
            } else {
                self.hidden = Some(Box::new(take_over.copy()));
            }
        }
        if (!take_over.ambient && self.ambient) || changed {
            self.ambient = take_over.ambient;
            changed = true;
        }
        if take_over.visible != self.visible {
            self.visible = take_over.visible;
            changed = true;
        }
        if take_over.show_icon != self.show_icon {
            self.show_icon = take_over.show_icon;
            changed = true;
        }
        changed
    }

    /// `tickDownDuration`, the hidden effects with it.
    pub fn tick_down(&mut self) {
        if let Some(hidden) = self.hidden.as_mut() {
            hidden.tick_down();
        }
        self.duration = self.map_duration(|d| d - 1);
    }

    /// `downgradeToHiddenEffect`: spent, the hidden effect comes back.
    /// Returns whether it did.
    pub fn downgrade_to_hidden(&mut self) -> bool {
        if self.duration != 0 {
            return false;
        }
        let Some(hidden) = self.hidden.take() else { return false };
        let hidden = *hidden;
        self.duration = hidden.duration;
        self.amplifier = hidden.amplifier;
        self.ambient = hidden.ambient;
        self.visible = hidden.visible;
        self.show_icon = hidden.show_icon;
        self.hidden = hidden.hidden;
        true
    }

    /// `BlendState.hasEffect`: it counts as on until its blend-out lead.
    fn blending_in(&self) -> bool {
        !self.ends_within(self.effect.blend_durations().2)
    }

    /// `BlendState.setImmediate` (`skipBlending`).
    pub fn skip_blending(&mut self) {
        let factor = if self.blending_in() { 1.0 } else { 0.0 };
        self.blend = Blend { factor, previous: factor };
    }

    /// `BlendState.tick`: a step toward on or off, a blend duration's
    /// share a tick.
    pub fn tick_blend(&mut self) {
        self.blend.previous = self.blend.factor;
        let on = self.blending_in();
        let target = if on { 1.0 } else { 0.0 };
        if self.blend.factor != target {
            let (blend_in, blend_out, _) = self.effect.blend_durations();
            let duration = if on { blend_in } else { blend_out };
            if duration == 0 {
                self.blend.factor = target;
            } else {
                let step = 1.0 / duration as f32;
                self.blend.factor += (target - self.blend.factor).clamp(-step, step);
            }
        }
    }

    /// `getBlendFactor`.
    pub fn blend_factor(&self, partial: f32) -> f32 {
        self.blend.previous + (self.blend.factor - self.blend.previous) * partial
    }

    /// `MobEffectInstance.compareTo`, the order the HUD and the inventory
    /// list effects in: not ambient first, then finite, shorter and by
    /// colour; long or all-ambient ones by ambience and colour alone.
    pub fn compare(&self, other: &Self) -> std::cmp::Ordering {
        let cut_off = 32147;
        let colour = self.effect.color().cmp(&other.effect.color());
        if (self.duration <= cut_off || other.duration <= cut_off) && (!self.ambient || !other.ambient) {
            self.ambient
                .cmp(&other.ambient)
                .then(self.infinite().cmp(&other.infinite()))
                .then(self.duration.cmp(&other.duration))
                .then(colour)
        } else {
            self.ambient.cmp(&other.ambient).then(colour)
        }
    }

    /// The instance as saved (`MobEffectInstance.CODEC`).
    pub fn to_json(&self) -> Value {
        let mut value = self.details_json();
        value["id"] = json!(self.effect.id());
        value
    }

    fn details_json(&self) -> Value {
        let mut value = json!({
            "amplifier": self.amplifier,
            "duration": self.duration,
            "ambient": self.ambient,
            "show_particles": self.visible,
            "show_icon": self.show_icon,
        });
        if let Some(hidden) = &self.hidden {
            value["hidden_effect"] = hidden.details_json();
        }
        value
    }

    /// An instance as saved, its details defaulting as the codec's do.
    pub fn from_json(value: &Value) -> Option<Self> {
        let effect = MobEffect::from_id(value["id"].as_str()?)?;
        Some(Self::details_from_json(effect, value))
    }

    fn details_from_json(effect: MobEffect, value: &Value) -> Self {
        let visible = value["show_particles"].as_bool().unwrap_or(true);
        let mut instance = Self::with_flags(
            effect,
            value["duration"].as_i64().unwrap_or(0) as i32,
            value["amplifier"].as_i64().unwrap_or(0) as i32,
            value["ambient"].as_bool().unwrap_or(false),
            visible,
            value["show_icon"].as_bool().unwrap_or(visible),
        );
        if value["hidden_effect"].is_object() {
            instance.hidden = Some(Box::new(Self::details_from_json(effect, &value["hidden_effect"])));
        }
        instance
    }
}

/// What `ActiveEffects::add` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Added {
    /// Nothing changed.
    Unchanged,
    /// A new effect (`onEffectAdded`).
    New,
    /// The effect it had changed (`onEffectUpdated`).
    Updated,
}

/// A living entity's active effects (`activeEffects`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActiveEffects {
    list: Vec<EffectInstance>,
}

impl ActiveEffects {
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &EffectInstance> {
        self.list.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut EffectInstance> {
        self.list.iter_mut()
    }

    /// `hasEffect`.
    pub fn has(&self, effect: MobEffect) -> bool {
        self.list.iter().any(|e| e.effect == effect)
    }

    /// `getEffect`.
    pub fn get(&self, effect: MobEffect) -> Option<&EffectInstance> {
        self.list.iter().find(|e| e.effect == effect)
    }

    /// `getEffect`, to change.
    pub fn get_mut(&mut self, effect: MobEffect) -> Option<&mut EffectInstance> {
        self.list.iter_mut().find(|e| e.effect == effect)
    }

    /// The level of an effect it has.
    pub fn amplifier(&self, effect: MobEffect) -> Option<i32> {
        self.get(effect).map(|e| e.amplifier)
    }

    /// `LivingEntity.addEffect` once `canBeAffected` has agreed: a new
    /// effect, or an update of the one it has.
    pub fn add(&mut self, instance: EffectInstance) -> Added {
        match self.list.iter_mut().find(|e| e.effect == instance.effect) {
            None => {
                self.list.push(instance);
                Added::New
            }
            Some(effect) => {
                if effect.update(&instance) {
                    Added::Updated
                } else {
                    Added::Unchanged
                }
            }
        }
    }

    /// `removeEffect`.
    pub fn remove(&mut self, effect: MobEffect) -> Option<EffectInstance> {
        let index = self.list.iter().position(|e| e.effect == effect)?;
        Some(self.list.remove(index))
    }

    /// `removeAllEffects`: whether there were any.
    pub fn clear(&mut self) -> bool {
        let any = !self.list.is_empty();
        self.list.clear();
        any
    }

    /// Keeps the effects `f` keeps, in order.
    pub fn retain_mut(&mut self, f: impl FnMut(&mut EffectInstance) -> bool) {
        self.list.retain_mut(f);
    }

    /// The value of an attribute from `base` with the effects' modifiers
    /// (`AttributeInstance.calculateValue`): the additions, then each
    /// multiplier in turn. `others` are the entity's own modifiers, which
    /// come first.
    pub fn attribute(&self, attribute: Attribute, base: f64, others: &[(f64, Operation)]) -> f64 {
        let mut modifiers: Vec<(f64, Operation)> = others.to_vec();
        for effect in &self.list {
            for &(target, amount, operation) in effect.effect.modifiers() {
                if target == attribute {
                    modifiers.push((f64::from(amount) * f64::from(effect.amplifier + 1), operation));
                }
            }
        }
        let mut value = base;
        for &(amount, operation) in &modifiers {
            if operation == Operation::AddValue {
                value += amount;
            }
        }
        for &(amount, operation) in &modifiers {
            if operation == Operation::AddMultipliedTotal {
                value *= 1.0 + amount;
            }
        }
        value
    }

    /// `MobEffectUtil.hasDigSpeed` and `getDigSpeedAmplification`: haste's
    /// or conduit power's level, the greater.
    pub fn dig_speed_amplification(&self) -> Option<i32> {
        match (self.amplifier(Haste), self.amplifier(ConduitPower)) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0).max(b.unwrap_or(0))),
        }
    }

    /// `Player.getDestroySpeed`'s effects on a speed: haste or conduit
    /// power 20% faster a level, mining fatigue 0.3 times a level.
    pub fn destroy_speed(&self, mut speed: f32) -> f32 {
        if let Some(amplifier) = self.dig_speed_amplification() {
            speed *= 1.0 + (amplifier + 1) as f32 * 0.2;
        }
        if let Some(amplifier) = self.amplifier(MiningFatigue) {
            speed *= 0.3_f64.powi(amplifier + 1) as f32;
        }
        speed
    }

    /// `getModifiedSwingDuration`: haste and conduit power shorten a
    /// swing a tick a level, mining fatigue lengthens it two.
    pub fn swing_duration(&self, duration: i32) -> i32 {
        if let Some(amplifier) = self.dig_speed_amplification() {
            duration - (1 + amplifier)
        } else if let Some(amplifier) = self.amplifier(MiningFatigue) {
            duration + (1 + amplifier) * 2
        } else {
            duration
        }
    }

    /// `getJumpBoostPower`.
    pub fn jump_boost_power(&self) -> f32 {
        self.amplifier(JumpBoost).map_or(0.0, |a| 0.1 * (a as f32 + 1.0))
    }

    /// `MobEffectUtil.hasWaterBreathing`.
    pub fn water_breathing(&self) -> bool {
        self.has(WaterBreathing) || self.has(ConduitPower) || self.has(BreathOfTheNautilus)
    }

    /// `MobEffectUtil.shouldEffectsRefillAirsupply`.
    pub fn refill_air(&self) -> bool {
        !self.has(BreathOfTheNautilus) || self.has(WaterBreathing) || self.has(ConduitPower)
    }

    /// `areAllEffectsAmbient`: those with particles all are.
    pub fn all_ambient(&self) -> bool {
        self.list.iter().all(|e| !e.visible || e.ambient)
    }

    /// The effects as saved (`active_effects`).
    pub fn to_json(&self) -> Value {
        Value::Array(self.list.iter().map(EffectInstance::to_json).collect())
    }

    /// Effects as saved (`readAdditionalSaveData`), skipping unknown ones.
    /// They are on from the start, with no fade in (the login packets'
    /// `skipBlending`).
    pub fn from_json(value: &Value) -> Self {
        let mut list: Vec<EffectInstance> = value.as_array().map_or_else(Vec::new, |list| list.iter().filter_map(EffectInstance::from_json).collect());
        list.iter_mut().for_each(EffectInstance::skip_blending);
        Self { list }
    }
}

/// A registered potion (`Potions`): its ID path, its name for the item
/// names (`Potion.name`) and its effects (effect, ticks, level).
pub struct Potion {
    pub id: &'static str,
    pub name: &'static str,
    pub effects: &'static [(MobEffect, i32, i32)],
}

/// `Potions`, in registry order.
pub const POTIONS: &[Potion] = &[
    potion("water", "water", &[]),
    potion("mundane", "mundane", &[]),
    potion("thick", "thick", &[]),
    potion("awkward", "awkward", &[]),
    potion("night_vision", "night_vision", &[(NightVision, 3600, 0)]),
    potion("long_night_vision", "night_vision", &[(NightVision, 9600, 0)]),
    potion("invisibility", "invisibility", &[(Invisibility, 3600, 0)]),
    potion("long_invisibility", "invisibility", &[(Invisibility, 9600, 0)]),
    potion("leaping", "leaping", &[(JumpBoost, 3600, 0)]),
    potion("long_leaping", "leaping", &[(JumpBoost, 9600, 0)]),
    potion("strong_leaping", "leaping", &[(JumpBoost, 1800, 1)]),
    potion("fire_resistance", "fire_resistance", &[(FireResistance, 3600, 0)]),
    potion("long_fire_resistance", "fire_resistance", &[(FireResistance, 9600, 0)]),
    potion("swiftness", "swiftness", &[(Speed, 3600, 0)]),
    potion("long_swiftness", "swiftness", &[(Speed, 9600, 0)]),
    potion("strong_swiftness", "swiftness", &[(Speed, 1800, 1)]),
    potion("slowness", "slowness", &[(Slowness, 1800, 0)]),
    potion("long_slowness", "slowness", &[(Slowness, 4800, 0)]),
    potion("strong_slowness", "slowness", &[(Slowness, 400, 3)]),
    potion("turtle_master", "turtle_master", &[(Slowness, 400, 3), (Resistance, 400, 2)]),
    potion("long_turtle_master", "turtle_master", &[(Slowness, 800, 3), (Resistance, 800, 2)]),
    potion("strong_turtle_master", "turtle_master", &[(Slowness, 400, 5), (Resistance, 400, 3)]),
    potion("water_breathing", "water_breathing", &[(WaterBreathing, 3600, 0)]),
    potion("long_water_breathing", "water_breathing", &[(WaterBreathing, 9600, 0)]),
    potion("healing", "healing", &[(InstantHealth, 1, 0)]),
    potion("strong_healing", "healing", &[(InstantHealth, 1, 1)]),
    potion("harming", "harming", &[(InstantDamage, 1, 0)]),
    potion("strong_harming", "harming", &[(InstantDamage, 1, 1)]),
    potion("poison", "poison", &[(Poison, 900, 0)]),
    potion("long_poison", "poison", &[(Poison, 1800, 0)]),
    potion("strong_poison", "poison", &[(Poison, 432, 1)]),
    potion("regeneration", "regeneration", &[(Regeneration, 900, 0)]),
    potion("long_regeneration", "regeneration", &[(Regeneration, 1800, 0)]),
    potion("strong_regeneration", "regeneration", &[(Regeneration, 450, 1)]),
    potion("strength", "strength", &[(Strength, 3600, 0)]),
    potion("long_strength", "strength", &[(Strength, 9600, 0)]),
    potion("strong_strength", "strength", &[(Strength, 1800, 1)]),
    potion("weakness", "weakness", &[(Weakness, 1800, 0)]),
    potion("long_weakness", "weakness", &[(Weakness, 4800, 0)]),
    potion("luck", "luck", &[(Luck, 6000, 0)]),
    potion("slow_falling", "slow_falling", &[(SlowFalling, 1800, 0)]),
    potion("long_slow_falling", "slow_falling", &[(SlowFalling, 4800, 0)]),
    potion("wind_charged", "wind_charged", &[(WindCharged, 3600, 0)]),
    potion("weaving", "weaving", &[(Weaving, 3600, 0)]),
    potion("oozing", "oozing", &[(Oozing, 3600, 0)]),
    potion("infested", "infested", &[(Infested, 3600, 0)]),
];

const fn potion(id: &'static str, name: &'static str, effects: &'static [(MobEffect, i32, i32)]) -> Potion {
    Potion { id, name, effects }
}

impl Potion {
    /// The potion a registry ID names, with or without its namespace.
    pub fn from_id(id: &str) -> Option<&'static Potion> {
        let path = id.strip_prefix("minecraft:").unwrap_or(id);
        POTIONS.iter().find(|p| p.id == path)
    }

    /// `Potion.getEffects`.
    pub fn effects(&self) -> impl Iterator<Item = EffectInstance> + '_ {
        self.effects.iter().map(|&(effect, duration, amplifier)| EffectInstance::new(effect, duration, amplifier))
    }
}

/// `PotionContents`: a stack's potion, custom colour and custom effects,
/// from its `potion_contents` component (a bare potion ID, or the full
/// form).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PotionContents {
    pub potion: Option<&'static Potion>,
    pub custom_color: Option<u32>,
    pub custom_effects: Vec<EffectInstance>,
}

impl std::fmt::Debug for Potion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Potion({})", self.id)
    }
}

impl PartialEq for Potion {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

/// `PotionContents.getColor`'s default: the colour of an empty bottle's
/// contents.
pub const BASE_POTION_COLOR: u32 = 0x385DC6;

impl PotionContents {
    /// The contents of a stack's components, if it has any.
    pub fn from_components(components: Option<&Value>) -> Option<Self> {
        let contents = components?.get("minecraft:potion_contents")?;
        if let Some(id) = contents.as_str() {
            return Some(Self { potion: Potion::from_id(id), ..Self::default() });
        }
        Some(Self {
            potion: contents["potion"].as_str().and_then(Potion::from_id),
            custom_color: contents["custom_color"].as_i64().map(|c| c as u32 & 0xFFFFFF),
            custom_effects: contents["custom_effects"]
                .as_array()
                .map_or_else(Vec::new, |list| list.iter().filter_map(EffectInstance::from_json).collect()),
        })
    }

    /// `getAllEffects`: the potion's, then the custom ones.
    pub fn all_effects(&self) -> Vec<EffectInstance> {
        let mut effects: Vec<EffectInstance> = self.potion.map_or_else(Vec::new, |p| p.effects().collect());
        effects.extend(self.custom_effects.iter().map(EffectInstance::copy));
        effects
    }

    /// `forEachEffect` at a duration scale (a lingering potion's 0.25).
    pub fn scaled_effects(&self, scale: f32) -> Vec<EffectInstance> {
        self.all_effects().iter().map(|e| e.with_scaled_duration(scale)).collect()
    }

    /// `getColor`: the custom colour, else the effects' mix, else the
    /// water's blue.
    pub fn color(&self) -> u32 {
        self.custom_color.or_else(|| mixed_color(&self.all_effects())).unwrap_or(BASE_POTION_COLOR)
    }
}

/// `PotionContents.getColorOptional`: the visible effects' colours
/// averaged, each weighted by its level.
pub fn mixed_color(effects: &[EffectInstance]) -> Option<u32> {
    let (mut red, mut green, mut blue, mut weight) = (0, 0, 0, 0);
    for effect in effects.iter().filter(|e| e.visible) {
        let color = effect.effect.color() as i32;
        let level = effect.amplifier + 1;
        red += level * (color >> 16 & 0xFF);
        green += level * (color >> 8 & 0xFF);
        blue += level * (color & 0xFF);
        weight += level;
    }
    (weight != 0).then(|| (((red / weight) << 16) | ((green / weight) << 8) | (blue / weight)) as u32)
}

/// `SuspiciousStewEffects`: a stew's effects (each `duration` defaulting
/// to 160 ticks).
pub fn stew_effects(components: Option<&Value>) -> Vec<EffectInstance> {
    let Some(list) = components.and_then(|c| c["minecraft:suspicious_stew_effects"].as_array()) else { return Vec::new() };
    list.iter()
        .filter_map(|entry| {
            let effect = MobEffect::from_id(entry["id"].as_str()?)?;
            Some(EffectInstance::new(effect, entry["duration"].as_i64().unwrap_or(160) as i32, 0))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stronger_shorter_effect_hides_the_longer_one_until_it_runs_out() {
        let mut effects = ActiveEffects::default();
        effects.add(EffectInstance::new(Speed, 100, 0));
        assert_eq!(effects.add(EffectInstance::new(Speed, 3, 1)), Added::Updated);
        for _ in 0..3 {
            effects.retain_mut(|e| {
                e.tick_down();
                e.downgrade_to_hidden();
                e.has_remaining_duration()
            });
        }
        let speed = effects.get(Speed).unwrap();
        assert_eq!((speed.amplifier, speed.duration, speed.hidden.is_none()), (0, 97, true));
    }

    #[test]
    fn intervals_halve_a_level() {
        assert!(Regeneration.applies_this_tick(100, 0) && !Regeneration.applies_this_tick(75, 0));
        assert!(Regeneration.applies_this_tick(75, 1) && !Regeneration.applies_this_tick(74, 1));
        assert!(Wither.applies_this_tick(20, 1) && Poison.applies_this_tick(7, 5));
        assert!(Poison.applies_this_tick(12, 1) && !Poison.applies_this_tick(13, 1));
    }

    #[test]
    fn attributes_and_dig_speed() {
        let mut effects = ActiveEffects::default();
        effects.add(EffectInstance::new(Speed, 100, 1));
        effects.add(EffectInstance::new(Strength, 100, 1));
        let speed = effects.attribute(Attribute::MovementSpeed, 0.1, &[]);
        assert_eq!(speed, 0.1 * (1.0 + f64::from(0.2_f32) * 2.0));
        assert_eq!(effects.attribute(Attribute::AttackDamage, 1.0, &[]), 7.0);
        effects.add(EffectInstance::new(MiningFatigue, 100, 2));
        assert_eq!(effects.destroy_speed(1.0), 0.3_f64.powi(3) as f32);
        assert_eq!(effects.swing_duration(6), 12);
        effects.add(EffectInstance::new(Haste, 100, 1));
        assert_eq!(effects.swing_duration(6), 4);
    }

    #[test]
    fn potion_colours_mix_by_level_and_durations_scale_down() {
        let turtle = Potion::from_id("minecraft:turtle_master").unwrap();
        let effects: Vec<_> = turtle.effects().collect();
        // Slowness at level 4 and resistance at level 3, averaged.
        let (s, r) = (9154528_i32, 9520880_i32);
        let mix = |shift: i32| (4 * (s >> shift & 0xFF) + 3 * (r >> shift & 0xFF)) / 7;
        assert_eq!(mixed_color(&effects), Some(((mix(16) << 16) | (mix(8) << 8) | mix(0)) as u32));
        assert_eq!(effects[0].with_scaled_duration(0.25).duration, 100);
        assert_eq!(EffectInstance::new(InstantHealth, 1, 0).with_scaled_duration(0.25).duration, 1);
        let contents = PotionContents::from_components(Some(&json!({"minecraft:potion_contents": "minecraft:water"}))).unwrap();
        assert_eq!(contents.color(), BASE_POTION_COLOR);
    }

    #[test]
    fn saved_effects_round_trip() {
        let mut instance = EffectInstance::with_flags(Absorption, 2400, 3, true, false, true);
        instance.hidden = Some(Box::new(EffectInstance::new(Absorption, 6000, 0)));
        instance.skip_blending();
        let mut effects = ActiveEffects::default();
        effects.add(instance);
        assert_eq!(ActiveEffects::from_json(&effects.to_json()), effects);
    }
}
