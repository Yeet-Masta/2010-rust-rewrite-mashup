//! Enchantments at work (26.3 `Enchantment`'s effect components, as
//! `EnchantmentHelper` runs them): every `enchantment/*.json`'s `slots` and
//! `effects`, read from the data pack and interpreted as vanilla's codecs
//! read them. Values come from `LevelBasedValue`s, are changed by
//! `EnchantmentValueEffect`s (add, multiply, set, remove_binomial,
//! exponential, all_of) and are gated by the loot conditions of each
//! `ConditionalEffect`, matched against the contexts `Enchantment` builds
//! (`damageContext`, `itemContext`, `entityContext`, `locationContext`).
//!
//! What an effect does to the world (`EnchantmentEntityEffect.apply`,
//! `EnchantmentLocationBasedEffect.onChangedBlock`) comes back as an
//! [`EntityEffect`] for the caller to carry out, since mobs, players and
//! blocks live in different crates; everything that is only a number is
//! worked out here. Unknown effect types come back as
//! [`EntityEffect::Other`] and unknown condition types fail, so a later
//! package can add them without changing the callers.
//!
//! One random source serves the conditions and the effects (vanilla draws
//! the conditions on the level's random and the effects mostly on the
//! entity's).
use crate::inventory::ItemStack;
use crate::item_catalog::{AttributeModifier, ModifierOperation};
use crate::rng::LegacyRandom;
use glam::DVec3;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// The draws effects and conditions make (`RandomSource`).
pub trait Draws {
    fn next_float(&mut self) -> f32;
    fn next_int(&mut self, bound: u32) -> u32;
    fn next_double(&mut self) -> f64;
    fn next_gaussian(&mut self) -> f64;
}

impl Draws for LegacyRandom {
    fn next_float(&mut self) -> f32 {
        LegacyRandom::next_float(self)
    }
    fn next_int(&mut self, bound: u32) -> u32 {
        LegacyRandom::next_int(self, bound)
    }
    fn next_double(&mut self) -> f64 {
        LegacyRandom::next_double(self)
    }
    fn next_gaussian(&mut self) -> f64 {
        LegacyRandom::next_gaussian(self)
    }
}

/// `EquipmentSlot`, in `EquipmentSlot.VALUES` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    MainHand,
    OffHand,
    Feet,
    Legs,
    Chest,
    Head,
    Body,
    Saddle,
}

impl Slot {
    pub const ALL: [Slot; 8] = [Self::MainHand, Self::OffHand, Self::Feet, Self::Legs, Self::Chest, Self::Head, Self::Body, Self::Saddle];

    /// `getSerializedName`.
    pub fn name(self) -> &'static str {
        match self {
            Self::MainHand => "mainhand",
            Self::OffHand => "offhand",
            Self::Feet => "feet",
            Self::Legs => "legs",
            Self::Chest => "chest",
            Self::Head => "head",
            Self::Body => "body",
            Self::Saddle => "saddle",
        }
    }

    /// `EquipmentSlotGroup.test`: `armor` is the humanoid armor slots.
    pub fn in_group(self, group: &str) -> bool {
        match group {
            "any" => true,
            "hand" => matches!(self, Self::MainHand | Self::OffHand),
            "armor" => matches!(self, Self::Feet | Self::Legs | Self::Chest | Self::Head),
            other => other == self.name(),
        }
    }
}

/// Who a targeted effect is for (`EnchantmentTarget`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Attacker,
    DamagingEntity,
    Victim,
}

impl Target {
    fn parse(name: Option<&str>) -> Option<Self> {
        match name? {
            "attacker" => Some(Self::Attacker),
            "damaging_entity" => Some(Self::DamagingEntity),
            "victim" => Some(Self::Victim),
            _ => None,
        }
    }
}

/// What the conditions may ask of an entity (`EntityPredicate`'s view of
/// it). The caller fills in what its entity has.
#[derive(Clone, Debug, Default)]
pub struct EntityFacts {
    /// Its type's id (`minecraft:player`, `minecraft:zombie`...).
    pub type_id: String,
    pub position: DVec3,
    /// `getBbWidth` and `getBbHeight`.
    pub width: f32,
    pub height: f32,
    /// `getKnownMovement`: the last tick's motion.
    pub movement: DVec3,
    pub fall_distance: f64,
    pub tick_count: i32,
    pub on_ground: bool,
    pub on_fire: bool,
    pub crouching: bool,
    pub sprinting: bool,
    pub swimming: bool,
    /// `EntityFlagsPredicate`'s flying: gliding, or a player's abilities
    /// flying.
    pub flying: bool,
    pub fall_flying: bool,
    pub in_water: bool,
    pub baby: bool,
    /// Riding something (`getVehicle` is set).
    pub passenger: bool,
    /// The block `getBlockPosBelowThatAffectsMyMovement` holds.
    pub block_below: Option<String>,
    /// A player's game mode and food level (`PlayerPredicate`).
    pub player: Option<PlayerFacts>,
    /// What it holds, for `minecraft:equipment`.
    pub main_hand: Option<ItemStack>,
    /// The block at its position and whether that sees the sky
    /// (`LocationPredicate`'s `block` and `can_see_sky`).
    pub block_at: Option<String>,
    pub sees_sky: bool,
    /// Its level's `isRaining` and `isThundering` (`weather_check`).
    pub raining: bool,
    pub thundering: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PlayerFacts {
    /// `survival`, `creative`, `adventure` or `spectator`.
    pub game_mode: String,
    pub food_level: i32,
}

/// A `DamageSource` as the conditions see it.
#[derive(Clone, Copy, Debug)]
pub struct DamageFacts<'a> {
    /// The damage type's id (`minecraft:player_attack`...).
    pub damage_type: &'a str,
    /// `getEntity`: who caused it.
    pub attacker: Option<&'a EntityFacts>,
    /// `getDirectEntity`: what dealt it (an arrow, or the attacker).
    pub direct: Option<&'a EntityFacts>,
    /// `isDirect`: the causing entity dealt it itself.
    pub is_direct: bool,
}

/// What an entity effect does, for the caller to carry out on its world.
#[derive(Clone, Debug, PartialEq)]
pub enum EntityEffect {
    /// `ApplyMobEffect`: `addEffect` of an effect for some ticks.
    MobEffect { effect: String, duration: i32, amplifier: i32 },
    /// `DamageEntity`: `hurtServer` with this damage type, caused by the
    /// enchanted item's owner.
    Damage { damage_type: String, amount: f32 },
    /// `Ignite`: `igniteForSeconds`.
    Ignite { seconds: f32 },
    /// `ChangeItemDamage`: the enchanted item's `hurtAndBreak`.
    ItemDamage { amount: i32 },
    /// `ApplyExhaustion`: a player's `causeFoodExhaustion`.
    Exhaustion { amount: f32 },
    /// `ApplyEntityImpulse`: `direction` turned to the entity's look,
    /// scaled per axis and by `magnitude`, added to its motion.
    Impulse { direction: DVec3, scale: DVec3, magnitude: f32 },
    /// `PlaySoundEffect` (silent entities play nothing).
    Sound { event: String, position: DVec3, volume: f32, pitch: f32 },
    /// `SpawnParticlesEffect`: `sendParticles` with a count of 0, one
    /// particle moving at `velocity` (already times the speed).
    Particle { particle: Value, position: DVec3, velocity: DVec3 },
    /// `ExplodeEffect`: `ServerLevel.explode` with a
    /// `SimpleExplosionDamageCalculator`.
    Explode {
        position: DVec3,
        radius: f32,
        knockback_multiplier: Option<f32>,
        attribute_to_user: bool,
        damage_type: Option<String>,
        create_fire: bool,
        block_interaction: String,
        definition: Value,
    },
    /// `ReplaceDisk`: the blocks of a disk round `position`, [`disk`] lists
    /// them, set to `block` where `predicate` holds.
    ReplaceDisk { position: DVec3, offset: [i32; 3], radius: i32, height: i32, predicate: Option<Value>, block: Value, game_event: Option<String> },
    /// `ReplaceBlock`, `SetBlockProperties`, `SummonEntityEffect`,
    /// `RunFunction`: the definition, at the position.
    Other { kind: String, definition: Value, position: DVec3 },
}

/// An effect of `EnchantmentHelper.doPostAttackEffects`: on whom, and the
/// slot of the enchanted item (for its own wear).
#[derive(Clone, Debug, PartialEq)]
pub struct PostAttack {
    /// Whose item is enchanted: the victim's equipment or the attacker's
    /// weapon.
    pub enchanted: Target,
    pub affected: Target,
    pub slot: Slot,
    pub effect: EntityEffect,
}

/// A change `runLocationChangedEffects` makes.
#[derive(Clone, Debug, PartialEq)]
pub enum LocationChange {
    /// An attribute effect came on (`addTransientAttributeModifiers`).
    Add(AttributeModifier),
    /// It went off (`removeAttributeModifiers`).
    Remove(AttributeModifier),
    /// An entity effect ran (`onChangedBlock`), for the item in the slot.
    Effect(Slot, EntityEffect),
}

/// `LivingEntity.activeLocationDependentEnchantments`: per slot and
/// enchantment, the location effects (by index) that are on, with the
/// level they came on at.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocationEffects {
    active: Vec<(Slot, String, i32, Vec<usize>)>,
}

impl LocationEffects {
    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }
}

/// One enchantment's `slots` and `effects`.
#[derive(Clone, Debug, Default)]
struct Definition {
    slots: Vec<String>,
    effects: Map<String, Value>,
}

impl Definition {
    /// `Enchantment.matchingSlot`.
    fn matching_slot(&self, slot: Slot) -> bool {
        self.slots.iter().any(|group| slot.in_group(group))
    }

    /// `getEffects`: a list component's entries.
    fn list(&self, component: &str) -> &[Value] {
        self.effects.get(component).and_then(Value::as_array).map_or(&[], Vec::as_slice)
    }
}

/// The enchantments' effects and the tags their conditions name.
#[derive(Clone, Debug, Default)]
pub struct EnchantmentEffects {
    definitions: HashMap<String, Definition>,
    /// Members by registry and tag id (`entity_type/minecraft:undead`),
    /// nested tags expanded.
    tags: HashMap<String, HashSet<String>>,
}

/// The data pack registries whose tags the conditions read.
const TAG_REGISTRIES: [&str; 5] = ["item", "enchantment", "entity_type", "damage_type", "block"];

/// Whether a data JAR entry is one [`EnchantmentEffects::from_files`] reads.
pub fn wanted_file(name: &str) -> bool {
    name.starts_with("data/minecraft/enchantment/") || TAG_REGISTRIES.iter().any(|registry| name.starts_with(&format!("data/minecraft/tags/{registry}/")))
}

fn id_of(text: &str) -> String {
    if text.contains(':') { text.to_owned() } else { format!("minecraft:{text}") }
}

/// The `minecraft:enchantments` component of a stack, in its own order
/// (`ItemEnchantments`); an enchanted book's are stored, not had.
pub fn item_enchantments(stack: &ItemStack) -> Vec<(&str, i32)> {
    let Some(map) = stack.components.as_ref().and_then(|c| c.get("minecraft:enchantments")) else { return Vec::new() };
    // The older `{levels: {...}}` shape as well as the plain map.
    let map = map.get("levels").unwrap_or(map);
    map.as_object().map_or_else(Vec::new, |map| map.iter().filter_map(|(id, level)| Some((id.as_str(), level.as_i64()? as i32))).filter(|&(_, level)| level > 0).collect())
}

/// `LevelBasedValue.calculate`.
pub fn level_value(value: &Value, level: i32) -> f32 {
    if let Some(constant) = value.as_f64() {
        return constant as f32;
    }
    let field = |key: &str| value.get(key).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    match value.get("type").and_then(Value::as_str).unwrap_or("") {
        "minecraft:linear" => field("base") + field("per_level_above_first") * (level - 1) as f32,
        // `Mth.square(int)` then the float.
        "minecraft:levels_squared" => level.wrapping_mul(level) as f32 + field("added"),
        "minecraft:clamped" => level_value(&value["value"], level).clamp(field("min"), field("max")),
        "minecraft:fraction" => {
            let denominator = level_value(&value["denominator"], level);
            if denominator == 0.0 { 0.0 } else { level_value(&value["numerator"], level) / denominator }
        }
        "minecraft:lookup" => {
            let values = value["values"].as_array().map_or(&[][..], Vec::as_slice);
            match usize::try_from(level - 1).ok().and_then(|index| values.get(index)) {
                Some(entry) => entry.as_f64().unwrap_or(0.0) as f32,
                None => level_value(&value["fallback"], level),
            }
        }
        "minecraft:exponent" => f64::from(level_value(&value["base"], level)).powf(f64::from(level_value(&value["power"], level))) as f32,
        "minecraft:constant" => field("value"),
        _ => 0.0,
    }
}

/// `EnchantmentValueEffect.process`.
fn process_value(effect: &Value, level: i32, random: &mut dyn Draws, input: f32) -> f32 {
    match effect.get("type").and_then(Value::as_str).unwrap_or("") {
        "minecraft:add" => input + level_value(&effect["value"], level),
        "minecraft:multiply" => input * level_value(&effect["factor"], level),
        "minecraft:set" => level_value(&effect["value"], level),
        "minecraft:exponential" => (f64::from(input) * f64::from(level_value(&effect["base"], level)).powf(f64::from(level_value(&effect["exponent"], level)))) as f32,
        "minecraft:all_of" => effect["effects"].as_array().into_iter().flatten().fold(input, |value, inner| process_value(inner, level, random, value)),
        "minecraft:remove_binomial" => {
            // `RemoveBinomial`: a normal approximation for large counts,
            // else a draw per point.
            let (n, p) = (input, level_value(&effect["chance"], level));
            let mut removed = 0i64;
            if n > 128.0 && n * p >= 20.0 && n * (1.0 - p) >= 20.0 {
                let mean = f64::from(n * p).floor();
                let deviation = f64::from(n * p * (1.0 - p)).sqrt();
                removed = java_round_f64(mean + random.next_gaussian() * deviation).clamp(0, n as i64);
            } else {
                let mut y = 0.0f32;
                while y < n {
                    if random.next_float() < p {
                        removed += 1;
                    }
                    y += 1.0;
                }
            }
            n - removed as f32
        }
        _ => input,
    }
}

/// `Math.round(double)`.
fn java_round_f64(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

/// `Math.round(float)`.
fn java_round(value: f32) -> i32 {
    (f64::from(value) + 0.5).floor() as i32
}

/// `FloatProvider.sample`.
fn float_provider(value: &Value, random: &mut dyn Draws) -> f32 {
    if let Some(constant) = value.as_f64() {
        return constant as f32;
    }
    let field = |key: &str| value.get(key).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    match value.get("type").and_then(Value::as_str).unwrap_or("") {
        "minecraft:uniform" => random.next_float() * (field("max_exclusive") - field("min_inclusive")) + field("min_inclusive"),
        "minecraft:clamped_normal" => (field("mean") + random.next_gaussian() as f32 * field("deviation")).clamp(field("min"), field("max")),
        "minecraft:trapezoid" => {
            let (min, max, plateau) = (field("min"), field("max"), field("plateau"));
            let range = max - min;
            let bound = (range - plateau) / 2.0;
            let rest = range - bound;
            min + random.next_float() * rest + random.next_float() * bound
        }
        "minecraft:clamped" => float_provider(&value["source"], random).clamp(field("min"), field("max")),
        _ => field("value"),
    }
}

/// `MinMaxBounds.Doubles.matches` (a number is exact).
fn bounds(value: &Value, x: f64) -> bool {
    if let Some(exact) = value.as_f64() {
        return x == exact;
    }
    value.get("min").and_then(Value::as_f64).is_none_or(|min| x >= min) && value.get("max").and_then(Value::as_f64).is_none_or(|max| x <= max)
}

/// `matchesSqr`: the bounds squared against a squared length.
fn bounds_sqr(value: &Value, sqr: f64) -> bool {
    if let Some(exact) = value.as_f64() {
        return sqr == exact * exact;
    }
    let side = |key: &str| value.get(key).and_then(Value::as_f64).map(|b| if b < 0.0 { -(b * b) } else { b * b });
    side("min").is_none_or(|min| sqr >= min) && side("max").is_none_or(|max| sqr <= max)
}

/// The context a condition is matched in (`LootContext` with the
/// enchanted parameter sets).
#[derive(Clone, Copy)]
struct Context<'a> {
    /// `ENCHANTMENT_LEVEL`.
    level: i32,
    /// `THIS_ENTITY`.
    this: Option<&'a EntityFacts>,
    /// `DAMAGE_SOURCE`, with `ATTACKING_ENTITY` and
    /// `DIRECT_ATTACKING_ENTITY`.
    damage: Option<&'a DamageFacts<'a>>,
    /// `TOOL`.
    tool: Option<&'a ItemStack>,
    /// `ENCHANTMENT_ACTIVE`.
    active: Option<bool>,
}

impl<'a> Context<'a> {
    fn new(level: i32) -> Self {
        Self { level, this: None, damage: None, tool: None, active: None }
    }
}

/// The disk `ReplaceDisk` walks: the positions within the radius of the
/// point, at its offset block, in `BlockPos.betweenClosed` order.
pub fn disk(position: DVec3, offset: [i32; 3], radius: i32, height: i32) -> Vec<(i32, i32, i32)> {
    let center = (position.x.floor() as i32 + offset[0], position.y.floor() as i32 + offset[1], position.z.floor() as i32 + offset[2]);
    let top = center.1 + (height - 1).min(0);
    let mut out = Vec::new();
    for z in center.2 - radius..=center.2 + radius {
        for y in center.1.min(top)..=center.1.max(top) {
            for x in center.0 - radius..=center.0 + radius {
                // `distToCenterSqr` to the point at the block's middle height.
                let (dx, dy, dz) = (f64::from(x) + 0.5 - position.x, 0.0, f64::from(z) + 0.5 - position.z);
                if dx * dx + dy * dy + dz * dz < f64::from(radius * radius) {
                    out.push((x, y, z));
                }
            }
        }
    }
    out
}

impl EnchantmentEffects {
    /// The effects from the data pack's files, by their path in the JAR
    /// (`data/minecraft/enchantment/<id>.json` and the tags).
    pub fn from_files<'a>(files: impl IntoIterator<Item = (&'a str, &'a Value)>) -> Self {
        let mut definitions = HashMap::new();
        let mut raw: HashMap<String, Vec<String>> = HashMap::new();
        for (name, value) in files {
            let Some(path) = name.strip_prefix("data/minecraft/").and_then(|p| p.strip_suffix(".json")) else { continue };
            if let Some(id) = path.strip_prefix("enchantment/") {
                let slots = value["slots"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
                let effects = value.get("effects").and_then(Value::as_object).cloned().unwrap_or_else(Map::new);
                definitions.insert(format!("minecraft:{id}"), Definition { slots, effects });
            } else if let Some(tag) = path.strip_prefix("tags/") {
                let Some((registry, id)) = tag.split_once('/') else { continue };
                let values = value["values"].as_array().into_iter().flatten().filter_map(|v| v.as_str().or_else(|| v.get("id").and_then(Value::as_str))).map(str::to_owned).collect();
                raw.insert(format!("{registry}/minecraft:{id}"), values);
            }
        }
        let mut tags = HashMap::new();
        for key in raw.keys() {
            let (registry, _) = key.split_once('/').unwrap_or(("", ""));
            let mut members = HashSet::new();
            expand(&raw, registry, key, &mut members, 0);
            tags.insert(key.clone(), members);
        }
        Self { definitions, tags }
    }

    /// Whether any enchantment was read.
    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    /// A registry's tag holds the id (`Holder.is(TagKey)`).
    pub fn in_tag(&self, registry: &str, tag: &str, id: &str) -> bool {
        self.tags.get(&format!("{registry}/{}", id_of(tag))).is_some_and(|members| members.contains(id))
    }

    /// A holder set (`#tag`, an id, or a list of ids) holds the id.
    fn in_holder_set(&self, registry: &str, set: &Value, id: &str) -> bool {
        match set {
            Value::String(text) => match text.strip_prefix('#') {
                Some(tag) => self.in_tag(registry, tag, id),
                None => id_of(text) == id,
            },
            Value::Array(list) => list.iter().filter_map(Value::as_str).any(|entry| id_of(entry) == id),
            _ => false,
        }
    }

    /// A holder set's members, in order (`HolderSet.getRandomElement`
    /// draws among them).
    fn holder_members(&self, registry: &str, set: &Value) -> Vec<String> {
        match set {
            Value::String(text) => match text.strip_prefix('#') {
                Some(tag) => {
                    let mut members: Vec<String> = self.tags.get(&format!("{registry}/{}", id_of(tag))).map_or_else(Vec::new, |m| m.iter().cloned().collect());
                    members.sort();
                    members
                }
                None => vec![id_of(text)],
            },
            Value::Array(list) => list.iter().filter_map(Value::as_str).map(id_of).collect(),
            _ => Vec::new(),
        }
    }

    /// `runIterationOnItem`: the stack's enchantments with their
    /// definitions.
    fn on_item<'a>(&'a self, stack: &'a ItemStack) -> impl Iterator<Item = (&'a Definition, i32)> + 'a {
        item_enchantments(stack).into_iter().filter_map(|(id, level)| Some((self.definitions.get(id)?, level)))
    }

    /// `runIterationOnEquipment`: by slot in `EquipmentSlot.VALUES` order,
    /// each item's enchantments that act in that slot.
    fn on_equipment<'a>(&'a self, equipment: &'a [(Slot, &'a ItemStack)]) -> impl Iterator<Item = (Slot, &'a ItemStack, &'a str, &'a Definition, i32)> + 'a {
        Slot::ALL.into_iter().flat_map(move |slot| {
            equipment.iter().filter(move |(s, _)| *s == slot).flat_map(move |&(slot, stack)| {
                item_enchantments(stack).into_iter().filter_map(move |(id, level)| {
                    let definition = self.definitions.get(id)?;
                    definition.matching_slot(slot).then_some((slot, stack, id, definition, level))
                })
            })
        })
    }

    /// `getItemEnchantmentLevel`.
    pub fn level(&self, enchantment: &str, stack: &ItemStack) -> i32 {
        item_enchantments(stack).into_iter().find(|&(id, _)| id == enchantment).map_or(0, |(_, level)| level)
    }

    /// `getEnchantmentLevel`: the best level on the items in the slots the
    /// enchantment acts in.
    pub fn equipment_level(&self, enchantment: &str, equipment: &[(Slot, &ItemStack)]) -> i32 {
        let Some(definition) = self.definitions.get(enchantment) else { return 0 };
        equipment.iter().filter(|(slot, _)| definition.matching_slot(*slot)).map(|(_, stack)| self.level(enchantment, stack)).max().unwrap_or(0)
    }

    /// `EnchantmentHelper.has`: one of the stack's enchantments has the
    /// effect component.
    pub fn has(&self, stack: &ItemStack, component: &str) -> bool {
        self.on_item(stack).any(|(definition, _)| definition.effects.contains_key(component))
    }

    /// `EnchantmentHelper.hasTag`: one of the stack's enchantments is in
    /// the enchantment tag.
    pub fn has_tag(&self, stack: &ItemStack, tag: &str) -> bool {
        item_enchantments(stack).into_iter().any(|(id, _)| self.in_tag("enchantment", tag, id))
    }

    /// `getHighestLevel`: the highest-levelled enchantment with the
    /// component, its value and level (`pickHighestLevel` picks from a
    /// list by it).
    pub fn highest_level<'a>(&'a self, stack: &'a ItemStack, component: &str) -> Option<(&'a Value, i32)> {
        let mut found: Option<(&Value, i32)> = None;
        for (definition, level) in self.on_item(stack) {
            if found.is_none_or(|(_, best)| best < level) {
                if let Some(effect) = definition.effects.get(component) {
                    found = Some((effect, level));
                }
            }
        }
        found
    }

    /// `applyEffects` of a value component over the item's enchantments.
    fn modify<'c>(&self, stack: &ItemStack, component: &str, context: impl Fn(i32) -> Context<'c>, value: f32, random: &mut dyn Draws) -> f32 {
        let mut value = value;
        for (definition, level) in self.on_item(stack) {
            let cx = context(level);
            for entry in definition.list(component) {
                if self.requirements(entry, &cx, random) {
                    value = process_value(&entry["effect"], level, random, value);
                }
            }
        }
        value
    }

    /// An item-filtered value (`modifyItemFilteredCount`): the tool is
    /// `tool`.
    fn item_value(&self, stack: &ItemStack, tool: &ItemStack, component: &str, value: f32, random: &mut dyn Draws) -> f32 {
        self.modify(stack, component, |level| Context { tool: Some(tool), ..Context::new(level) }, value, random)
    }

    /// An entity-filtered value (`modifyEntityFilteredValue`).
    fn entity_value(&self, stack: &ItemStack, entity: &EntityFacts, component: &str, value: f32, random: &mut dyn Draws) -> f32 {
        self.modify(stack, component, |level| Context { this: Some(entity), ..Context::new(level) }, value, random)
    }

    /// A damage-filtered value (`modifyDamageFilteredValue`).
    fn damage_value(&self, stack: &ItemStack, victim: &EntityFacts, source: &DamageFacts, component: &str, value: f32, random: &mut dyn Draws) -> f32 {
        self.modify(stack, component, |level| Context { this: Some(victim), damage: Some(source), ..Context::new(level) }, value, random)
    }

    /// `processDurabilityChange`: the wear an item takes of `amount`.
    pub fn durability_change(&self, stack: &ItemStack, amount: i32, random: &mut dyn Draws) -> i32 {
        self.item_value(stack, stack, "minecraft:item_damage", amount as f32, random) as i32
    }

    /// `processAmmoUse`.
    pub fn ammo_use(&self, weapon: &ItemStack, ammo: &ItemStack, amount: i32, random: &mut dyn Draws) -> i32 {
        self.item_value(weapon, ammo, "minecraft:ammo_use", amount as f32, random) as i32
    }

    /// `getPiercingCount`.
    pub fn piercing_count(&self, weapon: &ItemStack, ammo: &ItemStack, random: &mut dyn Draws) -> i32 {
        (self.item_value(weapon, ammo, "minecraft:projectile_piercing", 0.0, random) as i32).max(0)
    }

    /// `processBlockExperience`.
    pub fn block_experience(&self, stack: &ItemStack, amount: i32, random: &mut dyn Draws) -> i32 {
        self.item_value(stack, stack, "minecraft:block_experience", amount as f32, random) as i32
    }

    /// `modifyDurabilityToRepairFromXp`.
    pub fn repair_with_xp(&self, stack: &ItemStack, durability: i32, random: &mut dyn Draws) -> i32 {
        (self.item_value(stack, stack, "minecraft:repair_with_xp", durability as f32, random) as i32).max(0)
    }

    /// `processMobExperience` for a living killer's equipment.
    pub fn mob_experience(&self, killer: &[(Slot, &ItemStack)], killed: &EntityFacts, amount: i32, random: &mut dyn Draws) -> i32 {
        let mut value = amount as f32;
        for (_, _, _, definition, level) in self.on_equipment(killer) {
            let cx = Context { this: Some(killed), ..Context::new(level) };
            for entry in definition.list("minecraft:mob_experience") {
                if self.requirements(entry, &cx, random) {
                    value = process_value(&entry["effect"], level, random, value);
                }
            }
        }
        value as i32
    }

    /// `processProjectileCount`.
    pub fn projectile_count(&self, weapon: &ItemStack, shooter: &EntityFacts, count: i32, random: &mut dyn Draws) -> i32 {
        (self.entity_value(weapon, shooter, "minecraft:projectile_count", count as f32, random) as i32).max(0)
    }

    /// `processProjectileSpread`.
    pub fn projectile_spread(&self, weapon: &ItemStack, shooter: &EntityFacts, angle: f32, random: &mut dyn Draws) -> f32 {
        self.entity_value(weapon, shooter, "minecraft:projectile_spread", angle, random).max(0.0)
    }

    /// `getTridentReturnToOwnerAcceleration`.
    pub fn trident_return_acceleration(&self, weapon: &ItemStack, trident: &EntityFacts, random: &mut dyn Draws) -> i32 {
        (self.entity_value(weapon, trident, "minecraft:trident_return_acceleration", 0.0, random) as i32).max(0)
    }

    /// `getFishingLuckBonus`.
    pub fn fishing_luck_bonus(&self, rod: &ItemStack, fisher: &EntityFacts, random: &mut dyn Draws) -> i32 {
        (self.entity_value(rod, fisher, "minecraft:fishing_luck_bonus", 0.0, random) as i32).max(0)
    }

    /// `getFishingTimeReduction`.
    pub fn fishing_time_reduction(&self, rod: &ItemStack, fisher: &EntityFacts, random: &mut dyn Draws) -> f32 {
        self.entity_value(rod, fisher, "minecraft:fishing_time_reduction", 0.0, random).max(0.0)
    }

    /// `modifyUnfilteredValue` over the item's enchantments.
    fn unfiltered(&self, stack: &ItemStack, component: &str, value: f32, random: &mut dyn Draws) -> f32 {
        let mut value = value;
        for (definition, level) in self.on_item(stack) {
            if let Some(effect) = definition.effects.get(component) {
                value = process_value(effect, level, random, value);
            }
        }
        value
    }

    /// `modifyCrossbowChargingTime`.
    pub fn crossbow_charge_time(&self, crossbow: &ItemStack, time: f32, random: &mut dyn Draws) -> f32 {
        self.unfiltered(crossbow, "minecraft:crossbow_charge_time", time, random).max(0.0)
    }

    /// `getTridentSpinAttackStrength`.
    pub fn trident_spin_attack_strength(&self, trident: &ItemStack, random: &mut dyn Draws) -> f32 {
        self.unfiltered(trident, "minecraft:trident_spin_attack_strength", 0.0, random)
    }

    /// `modifyDamage`: the weapon's damage on the victim.
    pub fn damage(&self, weapon: &ItemStack, victim: &EntityFacts, source: &DamageFacts, amount: f32, random: &mut dyn Draws) -> f32 {
        self.damage_value(weapon, victim, source, "minecraft:damage", amount, random)
    }

    /// `modifyFallBasedDamage`.
    pub fn fall_based_damage(&self, weapon: &ItemStack, victim: &EntityFacts, source: &DamageFacts, amount: f32, random: &mut dyn Draws) -> f32 {
        self.damage_value(weapon, victim, source, "minecraft:smash_damage_per_fallen_block", amount, random)
    }

    /// `modifyArmorEffectiveness`.
    pub fn armor_effectiveness(&self, weapon: &ItemStack, victim: &EntityFacts, source: &DamageFacts, fraction: f32, random: &mut dyn Draws) -> f32 {
        self.damage_value(weapon, victim, source, "minecraft:armor_effectiveness", fraction, random)
    }

    /// `modifyKnockback`.
    pub fn knockback(&self, weapon: &ItemStack, victim: &EntityFacts, source: &DamageFacts, amount: f32, random: &mut dyn Draws) -> f32 {
        self.damage_value(weapon, victim, source, "minecraft:knockback", amount, random)
    }

    /// `getDamageProtection`: the victim's equipment's protection against
    /// the source.
    pub fn damage_protection(&self, equipment: &[(Slot, &ItemStack)], victim: &EntityFacts, source: &DamageFacts, random: &mut dyn Draws) -> f32 {
        let mut value = 0.0;
        for (_, _, _, definition, level) in self.on_equipment(equipment) {
            let cx = Context { this: Some(victim), damage: Some(source), ..Context::new(level) };
            for entry in definition.list("minecraft:damage_protection") {
                if self.requirements(entry, &cx, random) {
                    value = process_value(&entry["effect"], level, random, value);
                }
            }
        }
        value
    }

    /// `isImmuneToDamage`.
    pub fn immune_to_damage(&self, equipment: &[(Slot, &ItemStack)], victim: &EntityFacts, source: &DamageFacts, random: &mut dyn Draws) -> bool {
        self.on_equipment(equipment).any(|(_, _, _, definition, level)| {
            let cx = Context { this: Some(victim), damage: Some(source), ..Context::new(level) };
            definition.list("minecraft:damage_immunity").iter().any(|entry| self.requirements(entry, &cx, random))
        })
    }

    /// `processEquipmentDropChance`: the victim's own equipment, then the
    /// living attacker's, change a drop chance.
    pub fn equipment_drop_chance(&self, victim_equipment: &[(Slot, &ItemStack)], attacker_equipment: Option<&[(Slot, &ItemStack)]>, victim: &EntityFacts, source: &DamageFacts, chance: f32, random: &mut dyn Draws) -> f32 {
        let mut value = chance;
        let sides = [(victim_equipment, Target::Victim)].into_iter().chain(attacker_equipment.map(|e| (e, Target::Attacker)));
        for (equipment, enchanted) in sides {
            for (_, _, _, definition, level) in self.on_equipment(equipment) {
                let cx = Context { this: Some(victim), damage: Some(source), ..Context::new(level) };
                for entry in definition.list("minecraft:equipment_drops") {
                    let targets = Target::parse(entry["enchanted"].as_str()) == Some(enchanted) && Target::parse(entry["affected"].as_str()) == Some(Target::Victim);
                    if targets && self.requirements(entry, &cx, random) {
                        value = process_value(&entry["effect"], level, random, value);
                    }
                }
            }
        }
        value
    }

    /// `forEachModifier(stack, EquipmentSlot)`: the `attributes` effects of
    /// the enchantments that act in the slot, each with the slot-suffixed
    /// id.
    pub fn attribute_modifiers(&self, stack: &ItemStack, slot: Slot) -> Vec<AttributeModifier> {
        let mut out = Vec::new();
        for (definition, level) in self.on_item(stack) {
            if !definition.matching_slot(slot) {
                continue;
            }
            for effect in definition.list("minecraft:attributes") {
                out.extend(attribute_modifier(effect, level, slot));
            }
        }
        out
    }

    /// The attribute modifiers of everything worn and held.
    pub fn equipment_modifiers(&self, equipment: &[(Slot, &ItemStack)]) -> Vec<AttributeModifier> {
        equipment.iter().flat_map(|&(slot, stack)| self.attribute_modifiers(stack, slot)).collect()
    }

    /// `getRandomItemWith`: among the equipped items the filter accepts,
    /// one entry per enchantment with the component that acts in its slot,
    /// one at random (`Util.getRandomSafe`).
    pub fn random_item_with(&self, component: &str, equipment: &[(Slot, &ItemStack)], filter: impl Fn(&ItemStack) -> bool, random: &mut dyn Draws) -> Option<Slot> {
        let mut items = Vec::new();
        for slot in Slot::ALL {
            for &(_, stack) in equipment.iter().filter(|(s, _)| *s == slot) {
                if !filter(stack) {
                    continue;
                }
                for (definition, _) in self.on_item(stack) {
                    if definition.effects.contains_key(component) && definition.matching_slot(slot) {
                        items.push(slot);
                    }
                }
            }
        }
        if items.is_empty() {
            return None;
        }
        Some(items[random.next_int(items.len() as u32) as usize])
    }

    /// `doPostAttackEffectsWithItemSource`: the victim's equipment's
    /// `victim`-enchanted effects, then the living attacker's weapon's
    /// `attacker`-enchanted ones (the weapon passed only when the source has
    /// a living attacker).
    pub fn post_attack(&self, victim_equipment: &[(Slot, &ItemStack)], weapon: Option<&ItemStack>, victim: &EntityFacts, source: &DamageFacts, random: &mut dyn Draws) -> Vec<PostAttack> {
        let mut out = Vec::new();
        for (slot, _, _, definition, level) in self.on_equipment(victim_equipment) {
            self.post_attack_effects(definition, level, slot, Target::Victim, victim, source, random, &mut out);
        }
        if let Some(weapon) = weapon {
            let main_hand = [(Slot::MainHand, weapon)];
            for (slot, _, _, definition, level) in self.on_equipment(&main_hand) {
                self.post_attack_effects(definition, level, slot, Target::Attacker, victim, source, random, &mut out);
            }
        }
        out
    }

    /// `Enchantment.doPostAttack` for one side.
    #[allow(clippy::too_many_arguments)]
    fn post_attack_effects(&self, definition: &Definition, level: i32, slot: Slot, side: Target, victim: &EntityFacts, source: &DamageFacts, random: &mut dyn Draws, out: &mut Vec<PostAttack>) {
        for entry in definition.list("minecraft:post_attack") {
            if Target::parse(entry["enchanted"].as_str()) != Some(side) {
                continue;
            }
            let cx = Context { this: Some(victim), damage: Some(source), ..Context::new(level) };
            if !self.requirements(entry, &cx, random) {
                continue;
            }
            let Some(affected) = Target::parse(entry["affected"].as_str()) else { continue };
            let target = match affected {
                Target::Attacker => source.attacker,
                Target::DamagingEntity => source.direct,
                Target::Victim => Some(victim),
            };
            // The effect works where its target stands.
            if let Some(target) = target {
                let mut effects = Vec::new();
                self.entity_effect(&entry["effect"], level, target, target.position, random, &mut effects);
                out.extend(effects.into_iter().map(|effect| PostAttack { enchanted: side, affected, slot, effect }));
            }
        }
    }

    /// `doPostPiercingAttackEffects`: the user's main-hand weapon.
    pub fn post_piercing_attack(&self, weapon: &ItemStack, user: &EntityFacts, random: &mut dyn Draws) -> Vec<EntityEffect> {
        let main_hand = [(Slot::MainHand, weapon)];
        let mut out = Vec::new();
        for (_, _, _, definition, level) in self.on_equipment(&main_hand) {
            self.entity_effects(definition.list("minecraft:post_piercing_attack"), level, user, random, &mut out);
        }
        out
    }

    /// `tickEffects`: every equipped enchantment's `tick` effects, with the
    /// slot of the item.
    pub fn tick(&self, equipment: &[(Slot, &ItemStack)], entity: &EntityFacts, random: &mut dyn Draws) -> Vec<(Slot, EntityEffect)> {
        let mut out = Vec::new();
        for (slot, _, _, definition, level) in self.on_equipment(equipment) {
            let mut effects = Vec::new();
            self.entity_effects(definition.list("minecraft:tick"), level, entity, random, &mut effects);
            out.extend(effects.into_iter().map(|effect| (slot, effect)));
        }
        out
    }

    /// `onProjectileSpawned`: the weapon's effects on its projectile.
    pub fn projectile_spawned(&self, weapon: &ItemStack, projectile: &EntityFacts, random: &mut dyn Draws) -> Vec<EntityEffect> {
        let mut out = Vec::new();
        for (definition, level) in self.on_item(weapon) {
            self.entity_effects(definition.list("minecraft:projectile_spawned"), level, projectile, random, &mut out);
        }
        out
    }

    /// `onHitBlock`: the weapon's effects where its projectile struck. The
    /// block's id stands in for `BLOCK_STATE`.
    pub fn hit_block(&self, weapon: &ItemStack, projectile: &EntityFacts, position: DVec3, random: &mut dyn Draws) -> Vec<EntityEffect> {
        let mut out = Vec::new();
        for (definition, level) in self.on_item(weapon) {
            let cx = Context { this: Some(projectile), ..Context::new(level) };
            for entry in definition.list("minecraft:hit_block") {
                if self.requirements(entry, &cx, random) {
                    self.entity_effect(&entry["effect"], level, projectile, position, random, &mut out);
                }
            }
        }
        out
    }

    /// `applyEffects` with an entity context, at the entity.
    fn entity_effects(&self, list: &[Value], level: i32, entity: &EntityFacts, random: &mut dyn Draws, out: &mut Vec<EntityEffect>) {
        let cx = Context { this: Some(entity), ..Context::new(level) };
        for entry in list {
            if self.requirements(entry, &cx, random) {
                self.entity_effect(&entry["effect"], level, entity, entity.position, random, out);
            }
        }
    }

    /// `runLocationChangedEffects` over the equipment: each enchantment's
    /// location effects come on or go off as their conditions now hold
    /// (with `enchantment_active_check` seeing whether each was on), and
    /// those on run again; enchantments no longer equipped in their slot
    /// go off.
    pub fn location_changed(&self, equipment: &[(Slot, &ItemStack)], entity: &EntityFacts, state: &mut LocationEffects, random: &mut dyn Draws) -> Vec<LocationChange> {
        let mut out = Vec::new();
        // What left its slot (`stopLocationBasedEffects` as the item
        // changed).
        let present: Vec<(Slot, &str, i32)> = equipment.iter().flat_map(|&(slot, stack)| item_enchantments(stack).into_iter().map(move |(id, level)| (slot, id, level))).collect();
        let mut kept = Vec::new();
        for (slot, id, level, effects) in std::mem::take(&mut state.active) {
            if present.iter().any(|&(s, i, l)| s == slot && i == id && l == level) {
                kept.push((slot, id, level, effects));
            } else if let Some(definition) = self.definitions.get(&id) {
                self.deactivate(definition, slot, level, &effects, &mut out);
            }
        }
        state.active = kept;
        for (slot, _, id, definition, level) in self.on_equipment(equipment) {
            let index = state.active.iter().position(|(s, i, _, _)| *s == slot && i == id);
            let mut active: Vec<usize> = index.map_or_else(Vec::new, |i| state.active[i].3.clone());
            for (n, entry) in definition.list("minecraft:location_changed").iter().enumerate() {
                let was_active = active.contains(&n);
                let cx = Context { this: Some(entity), active: Some(was_active), ..Context::new(level) };
                if self.requirements(entry, &cx, random) {
                    if !was_active {
                        active.push(n);
                    }
                    self.location_effect(&entry["effect"], level, slot, entity, !was_active, random, &mut out);
                } else if was_active {
                    active.retain(|&a| a != n);
                    self.location_off(&entry["effect"], level, slot, &mut out);
                }
            }
            match (index, active.is_empty()) {
                (Some(i), true) => {
                    state.active.remove(i);
                }
                (Some(i), false) => state.active[i].3 = active,
                (None, false) => state.active.push((slot, id.to_owned(), level, active)),
                (None, true) => {}
            }
        }
        out
    }

    /// `stopLocationBasedEffects`: everything on goes off.
    pub fn stop_location_effects(&self, state: &mut LocationEffects) -> Vec<LocationChange> {
        let mut out = Vec::new();
        for (slot, id, level, effects) in std::mem::take(&mut state.active) {
            if let Some(definition) = self.definitions.get(&id) {
                self.deactivate(definition, slot, level, &effects, &mut out);
            }
        }
        out
    }

    fn deactivate(&self, definition: &Definition, slot: Slot, level: i32, effects: &[usize], out: &mut Vec<LocationChange>) {
        for &n in effects {
            if let Some(entry) = definition.list("minecraft:location_changed").get(n) {
                self.location_off(&entry["effect"], level, slot, out);
            }
        }
    }

    /// `EnchantmentLocationBasedEffect.onChangedBlock`.
    #[allow(clippy::too_many_arguments)]
    fn location_effect(&self, effect: &Value, level: i32, slot: Slot, entity: &EntityFacts, became_active: bool, random: &mut dyn Draws, out: &mut Vec<LocationChange>) {
        match effect["type"].as_str().unwrap_or("") {
            "minecraft:all_of" => {
                for inner in effect["effects"].as_array().into_iter().flatten() {
                    self.location_effect(inner, level, slot, entity, became_active, random, out);
                }
            }
            "minecraft:attribute" => {
                if became_active {
                    out.extend(attribute_modifier(effect, level, slot).map(LocationChange::Add));
                }
            }
            _ => {
                let mut effects = Vec::new();
                self.entity_effect(effect, level, entity, entity.position, random, &mut effects);
                out.extend(effects.into_iter().map(|e| LocationChange::Effect(slot, e)));
            }
        }
    }

    /// `onDeactivated`: attribute effects take their modifier off.
    fn location_off(&self, effect: &Value, level: i32, slot: Slot, out: &mut Vec<LocationChange>) {
        match effect["type"].as_str().unwrap_or("") {
            "minecraft:all_of" => {
                for inner in effect["effects"].as_array().into_iter().flatten() {
                    self.location_off(inner, level, slot, out);
                }
            }
            "minecraft:attribute" => out.extend(attribute_modifier(effect, level, slot).map(LocationChange::Remove)),
            _ => {}
        }
    }

    /// `EnchantmentEntityEffect.apply` on `entity` at `position`.
    fn entity_effect(&self, effect: &Value, level: i32, entity: &EntityFacts, position: DVec3, random: &mut dyn Draws, out: &mut Vec<EntityEffect>) {
        let kind = effect["type"].as_str().unwrap_or("");
        match kind {
            "minecraft:all_of" => {
                for inner in effect["effects"].as_array().into_iter().flatten() {
                    self.entity_effect(inner, level, entity, position, random, out);
                }
            }
            "minecraft:apply_mob_effect" => {
                let members = self.holder_members("mob_effect", &effect["to_apply"]);
                if members.is_empty() {
                    return;
                }
                let chosen = members[random.next_int(members.len() as u32) as usize].clone();
                // `Mth.randomBetween` for the seconds and the amplifier.
                let between = |random: &mut dyn Draws, min: &Value, max: &Value| {
                    let (min, max) = (level_value(min, level), level_value(max, level));
                    random.next_float() * (max - min) + min
                };
                let duration = java_round(between(random, &effect["min_duration"], &effect["max_duration"]) * 20.0);
                let amplifier = java_round(between(random, &effect["min_amplifier"], &effect["max_amplifier"])).max(0);
                out.push(EntityEffect::MobEffect { effect: chosen, duration, amplifier });
            }
            "minecraft:damage_entity" => {
                let (min, max) = (level_value(&effect["min_damage"], level), level_value(&effect["max_damage"], level));
                let amount = random.next_float() * (max - min) + min;
                out.push(EntityEffect::Damage { damage_type: id_of(effect["damage_type"].as_str().unwrap_or("minecraft:generic")), amount });
            }
            "minecraft:ignite" => out.push(EntityEffect::Ignite { seconds: level_value(&effect["duration"], level) }),
            "minecraft:change_item_damage" => out.push(EntityEffect::ItemDamage { amount: level_value(&effect["amount"], level) as i32 }),
            "minecraft:apply_exhaustion" => out.push(EntityEffect::Exhaustion { amount: level_value(&effect["amount"], level) }),
            "minecraft:apply_impulse" => {
                let vector = |key: &str| {
                    let list = effect[key].as_array();
                    let at = |i: usize| list.and_then(|l| l.get(i)).and_then(Value::as_f64).unwrap_or(0.0);
                    DVec3::new(at(0), at(1), at(2))
                };
                out.push(EntityEffect::Impulse { direction: vector("direction"), scale: vector("coordinate_scale"), magnitude: level_value(&effect["magnitude"], level) });
            }
            "minecraft:play_sound" => {
                // One sound, or one per level up to the last.
                let sounds: Vec<&Value> = match &effect["sound"] {
                    Value::Array(list) => list.iter().collect(),
                    other => vec![other],
                };
                if sounds.is_empty() {
                    return;
                }
                let index = (level - 1).clamp(0, sounds.len() as i32 - 1) as usize;
                let event = sounds[index].as_str().or_else(|| sounds[index]["sound_id"].as_str()).map_or_else(String::new, id_of);
                let volume = float_provider(&effect["volume"], random);
                let pitch = float_provider(&effect["pitch"], random);
                out.push(EntityEffect::Sound { event, position, volume, pitch });
            }
            "minecraft:spawn_particles" => {
                // `PositionSource.getCoordinate` and
                // `VelocitySource.getVelocity`.
                let coordinate = |source: &Value, at: f64, center: f64, span: f32, random: &mut dyn Draws| {
                    let offset = source["offset"].as_f64().unwrap_or(0.0);
                    let scale = source["scale"].as_f64().unwrap_or(1.0);
                    let base = if source["type"].as_str() == Some("in_bounding_box") { center + (random.next_double() - 0.5) * f64::from(span) * scale } else { at };
                    base + offset
                };
                let velocity = |source: &Value, movement: f64, random: &mut dyn Draws| movement * source["movement_scale"].as_f64().unwrap_or(0.0) + f64::from(float_provider(source.get("base").unwrap_or(&Value::from(0.0)), random));
                let (horizontal, vertical) = (&effect["horizontal_position"], &effect["vertical_position"]);
                let x = coordinate(horizontal, position.x, position.x, entity.width, random);
                let y = coordinate(vertical, position.y, position.y + f64::from(entity.height) / 2.0, entity.height, random);
                let z = coordinate(horizontal, position.z, position.z, entity.width, random);
                let m = entity.movement;
                let vx = velocity(&effect["horizontal_velocity"], m.x, random);
                let vy = velocity(&effect["vertical_velocity"], m.y, random);
                let vz = velocity(&effect["horizontal_velocity"], m.z, random);
                let speed = effect.get("speed").map_or(0.0, |s| float_provider(s, random));
                out.push(EntityEffect::Particle { particle: effect["particle"].clone(), position: DVec3::new(x, y, z), velocity: DVec3::new(vx, vy, vz) * f64::from(speed) });
            }
            "minecraft:explode" => {
                let offset = effect["offset"].as_array().map_or(DVec3::ZERO, |l| DVec3::new(l.first().and_then(Value::as_f64).unwrap_or(0.0), l.get(1).and_then(Value::as_f64).unwrap_or(0.0), l.get(2).and_then(Value::as_f64).unwrap_or(0.0)));
                out.push(EntityEffect::Explode {
                    position: position + offset,
                    radius: level_value(&effect["radius"], level).max(0.0),
                    knockback_multiplier: effect.get("knockback_multiplier").map(|v| level_value(v, level)),
                    attribute_to_user: effect["attribute_to_user"].as_bool().unwrap_or(false),
                    damage_type: effect["damage_type"].as_str().map(id_of),
                    create_fire: effect["create_fire"].as_bool().unwrap_or(false),
                    block_interaction: effect["block_interaction"].as_str().unwrap_or("none").to_owned(),
                    definition: effect.clone(),
                });
            }
            "minecraft:replace_disk" => {
                let offset = effect["offset"].as_array().map_or([0; 3], |l| std::array::from_fn(|i| l.get(i).and_then(Value::as_i64).unwrap_or(0) as i32));
                out.push(EntityEffect::ReplaceDisk {
                    position,
                    offset,
                    radius: level_value(&effect["radius"], level) as i32,
                    height: level_value(&effect["height"], level) as i32,
                    predicate: effect.get("predicate").cloned(),
                    block: effect["block_state"].clone(),
                    game_event: effect["trigger_game_event"].as_str().map(id_of),
                });
            }
            _ => out.push(EntityEffect::Other { kind: kind.to_owned(), definition: effect.clone(), position }),
        }
    }

    /// `ConditionalEffect.matches`: no `requirements`, or they hold.
    fn requirements(&self, entry: &Value, cx: &Context, random: &mut dyn Draws) -> bool {
        entry.get("requirements").is_none_or(|condition| self.condition(condition, cx, random))
    }

    /// `LootItemCondition.test` for the conditions enchantments use.
    fn condition(&self, condition: &Value, cx: &Context, random: &mut dyn Draws) -> bool {
        if let Some(list) = condition.as_array() {
            return list.iter().all(|c| self.condition(c, cx, random));
        }
        let terms = || condition["terms"].as_array().map_or(&[][..], Vec::as_slice);
        match condition["type"].as_str().unwrap_or("") {
            "minecraft:all_of" => terms().iter().all(|c| self.condition(c, cx, random)),
            "minecraft:any_of" => terms().iter().any(|c| self.condition(c, cx, random)),
            "minecraft:inverted" => !self.condition(&condition["term"], cx, random),
            "minecraft:random_chance" => {
                let chance = self.number(&condition["chance"], cx, random);
                random.next_float() < chance
            }
            "minecraft:entity_properties" => {
                let entity = match condition["entity"].as_str().unwrap_or("this") {
                    "this" => cx.this,
                    "attacker" | "attacking_player" => cx.damage.and_then(|d| d.attacker),
                    "direct_attacker" => cx.damage.and_then(|d| d.direct),
                    _ => None,
                };
                self.entity_matches(condition.get("predicate"), entity)
            }
            "minecraft:damage_source_properties" => cx.damage.is_some_and(|source| self.damage_matches(&condition["predicate"], source)),
            "minecraft:match_tool" => cx.tool.is_some_and(|tool| self.item_matches(&condition["predicate"], tool)),
            "minecraft:enchantment_active_check" => cx.active == condition["active"].as_bool(),
            // `WeatherCheck`, and `LocationCheck` at the entity's place
            // (its offset unread): the level as the entity sees it.
            "minecraft:weather_check" => cx.this.is_some_and(|this| {
                condition["raining"].as_bool().is_none_or(|r| r == this.raining) && condition["thundering"].as_bool().is_none_or(|t| t == this.thundering)
            }),
            "minecraft:location_check" => cx.this.is_some_and(|this| self.location_matches(&condition["predicate"], this.block_at.as_deref(), Some(this.sees_sky))),
            _ => false,
        }
    }

    /// `NumberProvider.getFloat`.
    fn number(&self, value: &Value, cx: &Context, random: &mut dyn Draws) -> f32 {
        if let Some(constant) = value.as_f64() {
            return constant as f32;
        }
        match value["type"].as_str().unwrap_or("") {
            "minecraft:enchantment_level" => level_value(&value["amount"], cx.level),
            "minecraft:uniform" => {
                let (min, max) = (self.number(&value["min"], cx, random), self.number(&value["max"], cx, random));
                random.next_float() * (max - min) + min
            }
            "minecraft:binomial" => {
                let n = self.number(&value["n"], cx, random) as i32;
                let p = self.number(&value["p"], cx, random);
                (0..n).filter(|_| random.next_float() < p).count() as f32
            }
            _ => value["value"].as_f64().unwrap_or(0.0) as f32,
        }
    }

    /// `EntityPredicate.matches`: each of its parts holds; no entity
    /// matches nothing.
    fn entity_matches(&self, predicate: Option<&Value>, entity: Option<&EntityFacts>) -> bool {
        let Some(parts) = predicate.and_then(Value::as_object) else { return true };
        let Some(entity) = entity else { return false };
        parts.iter().all(|(key, value)| match key.as_str() {
            "minecraft:entity_type" | "type" => self.in_holder_set("entity_type", value, &entity.type_id),
            "minecraft:flags" | "flags" => flags_match(value, entity),
            "minecraft:movement" | "movement" => {
                // `MovementPredicate` on the known motion per second.
                let v = entity.movement * 20.0;
                let check = |key: &str, x: f64| value.get(key).is_none_or(|b| bounds(b, x));
                let check_sqr = |key: &str, sqr: f64| value.get(key).is_none_or(|b| bounds_sqr(b, sqr));
                check("x", v.x) && check("y", v.y) && check("z", v.z) && check_sqr("speed", v.length_squared()) && check_sqr("horizontal_speed", v.x * v.x + v.z * v.z) && check("vertical_speed", v.y.abs()) && check("fall_distance", entity.fall_distance)
            }
            "minecraft:movement_affected_by" | "movement_affected_by" => self.location_matches(value, entity.block_below.as_deref(), None),
            "minecraft:location" | "location" => self.location_matches(value, entity.block_at.as_deref(), Some(entity.sees_sky)),
            "minecraft:periodic_tick" | "periodic_tick" => value.as_i64().is_some_and(|period| period != 0 && i64::from(entity.tick_count) % period == 0),
            // Only "has a vehicle" is known of what is ridden.
            "minecraft:vehicle" | "vehicle" => entity.passenger && value.as_object().is_some_and(Map::is_empty),
            "minecraft:type_specific/player" => entity.player.as_ref().is_some_and(|player| {
                value.get("gamemode").is_none_or(|modes| modes.as_array().into_iter().flatten().any(|m| m.as_str() == Some(player.game_mode.as_str())))
                    && value.get("food").and_then(|f| f.get("level")).is_none_or(|b| bounds(b, f64::from(player.food_level)))
            }),
            "minecraft:equipment" | "equipment" => value.as_object().is_some_and(|slots| slots.iter().all(|(slot, item)| slot == "mainhand" && entity.main_hand.as_ref().is_some_and(|stack| self.item_matches(item, stack)))),
            _ => false,
        })
    }

    /// `LocationPredicate` on a block: only its block part is known.
    fn location_matches(&self, predicate: &Value, block: Option<&str>, sees_sky: Option<bool>) -> bool {
        let Some(parts) = predicate.as_object() else { return true };
        parts.iter().all(|(key, value)| match key.as_str() {
            "block" => block.is_some_and(|block| value.get("blocks").is_none_or(|set| self.in_holder_set("block", set, block)) && value.get("state").is_none()),
            "can_see_sky" => sees_sky.is_some_and(|sees| value.as_bool() == Some(sees)),
            _ => false,
        })
    }

    /// `DamageSourcePredicate.matches`.
    fn damage_matches(&self, predicate: &Value, source: &DamageFacts) -> bool {
        let tags_hold = predicate["tags"].as_array().into_iter().flatten().all(|tag| {
            let id = tag["id"].as_str().unwrap_or("");
            let expected = tag["expected"].as_bool().unwrap_or(true);
            let held = match id.strip_prefix('#') {
                Some(tag) => self.in_tag("damage_type", tag, source.damage_type),
                None => id_of(id) == source.damage_type,
            };
            held == expected
        });
        tags_hold
            && predicate.get("direct_entity").is_none_or(|p| self.entity_matches(Some(p), source.direct))
            && predicate.get("source_entity").is_none_or(|p| self.entity_matches(Some(p), source.attacker))
            && predicate.get("is_direct").and_then(Value::as_bool).is_none_or(|direct| direct == source.is_direct)
    }

    /// `ItemPredicate.test`: its items, count and enchantment predicates.
    pub fn item_matches(&self, predicate: &Value, stack: &ItemStack) -> bool {
        if predicate.get("items").is_some_and(|items| !self.in_holder_set("item", items, &stack.id)) {
            return false;
        }
        if predicate.get("count").is_some_and(|count| !bounds(count, f64::from(stack.count))) {
            return false;
        }
        let Some(parts) = predicate.get("predicates").and_then(Value::as_object) else { return true };
        parts.iter().all(|(key, wanted)| match key.as_str() {
            "minecraft:enchantments" => self.enchantments_match(wanted, &item_enchantments(stack)),
            "minecraft:stored_enchantments" => {
                let stored = stack.components.as_ref().and_then(|c| c.get("minecraft:stored_enchantments")).and_then(Value::as_object);
                let list: Vec<(&str, i32)> = stored.into_iter().flatten().filter_map(|(id, level)| Some((id.as_str(), level.as_i64()? as i32))).collect();
                self.enchantments_match(wanted, &list)
            }
            _ => false,
        })
    }

    /// `EnchantmentsPredicate`: each entry finds an enchantment of its set
    /// (any, without one) at a level in its bounds.
    fn enchantments_match(&self, wanted: &Value, list: &[(&str, i32)]) -> bool {
        wanted.as_array().into_iter().flatten().all(|entry| {
            list.iter().any(|&(id, level)| entry.get("enchantments").is_none_or(|set| self.in_holder_set("enchantment", set, id)) && entry.get("levels").is_none_or(|b| bounds(b, f64::from(level))))
        })
    }

    /// `BlockPredicate.test` for the predicates `ReplaceDisk` names, with
    /// the level's blocks as `block_at` reads them (id and properties).
    /// Entities are not known, so `unobstructed` holds.
    pub fn block_predicate(&self, predicate: &Value, pos: (i32, i32, i32), block_at: &dyn Fn((i32, i32, i32)) -> Option<crate::Block>) -> bool {
        let offset = |value: &Value| {
            let list = value["offset"].as_array();
            let at = |i: usize| list.and_then(|l| l.get(i)).and_then(Value::as_i64).unwrap_or(0) as i32;
            (pos.0 + at(0), pos.1 + at(1), pos.2 + at(2))
        };
        let id_at = |at| block_at(at).map_or_else(|| "minecraft:air".to_owned(), |b| b.id);
        let list = |key: &str| predicate[key].as_array().map_or(&[][..], Vec::as_slice);
        match predicate["type"].as_str().unwrap_or("") {
            "minecraft:all_of" => list("predicates").iter().all(|p| self.block_predicate(p, pos, block_at)),
            "minecraft:any_of" => list("predicates").iter().any(|p| self.block_predicate(p, pos, block_at)),
            "minecraft:not" => !self.block_predicate(&predicate["predicate"], pos, block_at),
            "minecraft:true" | "minecraft:unobstructed" => true,
            "minecraft:matching_blocks" => self.in_holder_set("block", &predicate["blocks"], &id_at(offset(predicate))),
            "minecraft:matching_block_tag" => self.in_tag("block", predicate["tag"].as_str().unwrap_or(""), &id_at(offset(predicate))),
            "minecraft:matching_fluids" => {
                // A source block's fluid is the still fluid itself; a
                // flowing one's is `flowing_*`.
                let fluid = block_at(offset(predicate)).and_then(|b| {
                    let still = match b.id.as_str() {
                        "minecraft:water" => "minecraft:water",
                        "minecraft:lava" => "minecraft:lava",
                        _ if b.property("waterlogged") == Some("true") => return Some("minecraft:water".to_owned()),
                        _ => return None,
                    };
                    let source = b.property("level").is_none_or(|level| level == "0");
                    Some(if source { still.to_owned() } else { still.replace("minecraft:", "minecraft:flowing_") })
                });
                fluid.is_some_and(|fluid| self.in_holder_set("fluid", &predicate["fluids"], &fluid))
            }
            _ => false,
        }
    }
}

/// `EntityFlagsPredicate.matches`.
fn flags_match(flags: &Value, entity: &EntityFacts) -> bool {
    let Some(flags) = flags.as_object() else { return true };
    flags.iter().all(|(key, expected)| {
        let actual = match key.as_str() {
            "is_on_ground" => entity.on_ground,
            "is_on_fire" => entity.on_fire,
            "is_crouching" => entity.crouching,
            "is_sprinting" => entity.sprinting,
            "is_swimming" => entity.swimming,
            "is_flying" => entity.flying,
            "is_baby" => entity.baby,
            "is_in_water" => entity.in_water,
            "is_fall_flying" => entity.fall_flying,
            _ => return false,
        };
        expected.as_bool() == Some(actual)
    })
}

/// `EnchantmentAttributeEffect.getModifier` for the slot.
fn attribute_modifier(effect: &Value, level: i32, slot: Slot) -> Option<AttributeModifier> {
    Some(AttributeModifier {
        attribute: id_of(effect["attribute"].as_str()?),
        id: format!("{}/{}", id_of(effect["id"].as_str()?), slot.name()),
        amount: f64::from(level_value(&effect["amount"], level)),
        operation: ModifierOperation::parse(effect["operation"].as_str()?)?,
        slot: slot.name().to_owned(),
    })
}

/// A tag's members with nested tags expanded.
fn expand(raw: &HashMap<String, Vec<String>>, registry: &str, key: &str, out: &mut HashSet<String>, depth: usize) {
    let Some(values) = raw.get(key) else { return };
    for value in values {
        match value.strip_prefix('#') {
            Some(nested) if depth < 16 => expand(raw, registry, &format!("{registry}/{}", id_of(nested)), out, depth + 1),
            Some(_) => {}
            None => {
                out.insert(id_of(value));
            }
        }
    }
}

/// An entity's equipment as its enchantments see it: the items by slot,
/// the attribute modifiers they give (`forEachModifier` as the equipment
/// changes) and those their location effects have on, and the entity
/// effects their ticks and location changes made, for the owner to carry
/// out.
#[derive(Clone, Debug, Default)]
pub struct Equipped {
    effects: Arc<EnchantmentEffects>,
    items: Vec<(Slot, ItemStack)>,
    modifiers: Vec<AttributeModifier>,
    location: LocationEffects,
    transient: Vec<AttributeModifier>,
    /// Effects to carry out, with the slot of the enchanted item.
    pub pending: Vec<(Slot, EntityEffect)>,
    random: LegacyRandom,
}

impl Equipped {
    /// The equipment as it now is; whether it changed
    /// (`collectEquipmentChanges`).
    pub fn set(&mut self, effects: &Arc<EnchantmentEffects>, items: Vec<(Slot, ItemStack)>) -> bool {
        if !Arc::ptr_eq(&self.effects, effects) {
            self.effects = effects.clone();
        } else if self.items == items {
            return false;
        }
        self.modifiers = items.iter().flat_map(|(slot, stack)| self.effects.attribute_modifiers(stack, *slot)).collect();
        self.items = items;
        true
    }

    /// The items by slot.
    pub fn items(&self) -> Vec<(Slot, &ItemStack)> {
        self.items.iter().map(|(slot, stack)| (*slot, stack)).collect()
    }

    pub fn effects(&self) -> &Arc<EnchantmentEffects> {
        &self.effects
    }

    /// The enchantments' draws.
    pub fn random(&mut self) -> &mut LegacyRandom {
        &mut self.random
    }

    /// The modifiers on an attribute, from the items and the location
    /// effects that are on.
    pub fn modifiers(&self, attribute: &str) -> impl Iterator<Item = &AttributeModifier> + Clone {
        let attribute = attribute.to_owned();
        self.modifiers.iter().chain(&self.transient).filter(move |m| m.attribute == attribute)
    }

    /// An attribute's value from its base with these modifiers, held to its
    /// range.
    pub fn attribute(&self, attribute: &str, base: f64, range: (f64, f64)) -> f64 {
        crate::item_catalog::attribute_value(base, self.modifiers(attribute), range)
    }

    /// `getDamageProtection`.
    pub fn protection(&mut self, victim: &EntityFacts, source: &DamageFacts) -> f32 {
        let items: Vec<(Slot, &ItemStack)> = self.items.iter().map(|(slot, stack)| (*slot, stack)).collect();
        self.effects.damage_protection(&items, victim, source, &mut self.random)
    }

    /// `isImmuneToDamage`.
    pub fn immune(&mut self, victim: &EntityFacts, source: &DamageFacts) -> bool {
        let items: Vec<(Slot, &ItemStack)> = self.items.iter().map(|(slot, stack)| (*slot, stack)).collect();
        self.effects.immune_to_damage(&items, victim, source, &mut self.random)
    }

    /// `doPostAttackEffects` on the wearer as the victim: its equipment's
    /// `victim`-enchanted effects (thorns).
    pub fn post_attack(&mut self, victim: &EntityFacts, source: &DamageFacts) -> Vec<PostAttack> {
        let items: Vec<(Slot, &ItemStack)> = self.items.iter().map(|(slot, stack)| (*slot, stack)).collect();
        self.effects.post_attack(&items, None, victim, source, &mut self.random)
    }

    /// `runLocationChangedEffects`: attribute effects come on and off,
    /// the rest go to `pending`.
    pub fn location_changed(&mut self, entity: &EntityFacts) {
        let items: Vec<(Slot, &ItemStack)> = self.items.iter().map(|(slot, stack)| (*slot, stack)).collect();
        let changes = self.effects.location_changed(&items, entity, &mut self.location, &mut self.random);
        self.apply(changes);
    }

    /// `stopLocationBasedEffects`.
    pub fn stop_location_effects(&mut self) {
        let changes = self.effects.stop_location_effects(&mut self.location);
        self.apply(changes);
    }

    fn apply(&mut self, changes: Vec<LocationChange>) {
        for change in changes {
            match change {
                LocationChange::Add(modifier) => {
                    self.transient.retain(|m| m.id != modifier.id || m.attribute != modifier.attribute);
                    self.transient.push(modifier);
                }
                LocationChange::Remove(modifier) => self.transient.retain(|m| m.id != modifier.id || m.attribute != modifier.attribute),
                LocationChange::Effect(slot, effect) => self.pending.push((slot, effect)),
            }
        }
    }

    /// `tickEffects`: the effects go to `pending`.
    pub fn tick(&mut self, entity: &EntityFacts) {
        let items: Vec<(Slot, &ItemStack)> = self.items.iter().map(|(slot, stack)| (*slot, stack)).collect();
        let effects = self.effects.tick(&items, entity, &mut self.random);
        self.pending.extend(effects);
    }
}

/// `CombatRules.getDamageAfterMagicAbsorb`: enchantment protection, at
/// most 20 points of 25.
pub fn damage_after_magic_absorb(damage: f32, protection: f32) -> f32 {
    damage * (1.0 - protection.clamp(0.0, 20.0) / 25.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn effects() -> EnchantmentEffects {
        let files = [
            ("data/minecraft/enchantment/sharpness.json", json!({"slots": ["mainhand"], "effects": {"minecraft:damage": [{"effect": {"type": "minecraft:add", "value": {"type": "minecraft:linear", "base": 1.0, "per_level_above_first": 0.5}}}]}})),
            ("data/minecraft/enchantment/smite.json", json!({"slots": ["mainhand"], "effects": {"minecraft:damage": [{"effect": {"type": "minecraft:add", "value": {"type": "minecraft:linear", "base": 2.5, "per_level_above_first": 2.5}}, "requirements": {"type": "minecraft:entity_properties", "entity": "this", "predicate": {"minecraft:entity_type": "#minecraft:sensitive_to_smite"}}}]}})),
            ("data/minecraft/enchantment/protection.json", json!({"slots": ["armor"], "effects": {"minecraft:damage_protection": [{"effect": {"type": "minecraft:add", "value": {"type": "minecraft:linear", "base": 1.0, "per_level_above_first": 1.0}}, "requirements": {"type": "minecraft:damage_source_properties", "predicate": {"tags": [{"expected": false, "id": "#minecraft:bypasses_invulnerability"}]}}}]}})),
            ("data/minecraft/enchantment/feather_falling.json", json!({"slots": ["armor"], "effects": {"minecraft:damage_protection": [{"effect": {"type": "minecraft:add", "value": {"type": "minecraft:linear", "base": 3.0, "per_level_above_first": 3.0}}, "requirements": {"type": "minecraft:damage_source_properties", "predicate": {"tags": [{"expected": true, "id": "#minecraft:is_fall"}]}}}]}})),
            ("data/minecraft/enchantment/unbreaking.json", json!({"slots": ["any"], "effects": {"minecraft:item_damage": [{"effect": {"type": "minecraft:remove_binomial", "chance": {"type": "minecraft:fraction", "numerator": {"type": "minecraft:linear", "base": 1.0, "per_level_above_first": 1.0}, "denominator": {"type": "minecraft:linear", "base": 2.0, "per_level_above_first": 1.0}}}}]}})),
            ("data/minecraft/enchantment/efficiency.json", json!({"slots": ["mainhand"], "effects": {"minecraft:attributes": [{"amount": {"type": "minecraft:levels_squared", "added": 1.0}, "attribute": "minecraft:mining_efficiency", "id": "minecraft:enchantment.efficiency", "operation": "add_value"}]}})),
            ("data/minecraft/tags/entity_type/sensitive_to_smite.json", json!({"values": ["#minecraft:undead"]})),
            ("data/minecraft/tags/entity_type/undead.json", json!({"values": ["minecraft:zombie", "minecraft:skeleton"]})),
            ("data/minecraft/tags/damage_type/is_fall.json", json!({"values": ["minecraft:fall"]})),
            ("data/minecraft/tags/damage_type/bypasses_invulnerability.json", json!({"values": ["minecraft:out_of_world"]})),
        ];
        EnchantmentEffects::from_files(files.iter().map(|(name, value)| (*name, value)))
    }

    fn enchanted(id: &str, enchantments: Value) -> ItemStack {
        let mut stack = ItemStack::new(id, 1);
        stack.components = Some(json!({ "minecraft:enchantments": enchantments }));
        stack
    }

    #[test]
    fn level_based_values_read_as_vanilla_computes_them() {
        assert_eq!(level_value(&json!({"type": "minecraft:linear", "base": 1.0, "per_level_above_first": 0.5}), 5), 3.0);
        assert_eq!(level_value(&json!({"type": "minecraft:levels_squared", "added": 1.0}), 3), 10.0);
        assert_eq!(level_value(&json!({"type": "minecraft:lookup", "values": [1.2, 1.75, 2.2], "fallback": {"type": "minecraft:linear", "base": 1.5, "per_level_above_first": 0.35}}), 2), 1.75);
        assert!((level_value(&json!({"type": "minecraft:lookup", "values": [1.2], "fallback": {"type": "minecraft:linear", "base": 1.5, "per_level_above_first": 0.35}}), 3) - 2.2).abs() < 1e-6);
        assert_eq!(level_value(&json!({"type": "minecraft:clamped", "value": 20.0, "min": 0.0, "max": 16.0}), 1), 16.0);
        assert_eq!(level_value(&json!({"type": "minecraft:fraction", "numerator": 1.0, "denominator": 0.0}), 1), 0.0);
    }

    #[test]
    fn damage_follows_the_victims_type_and_protection_the_source() {
        let effects = effects();
        let mut random = LegacyRandom::new(1);
        let sword = enchanted("minecraft:iron_sword", json!({"minecraft:sharpness": 3, "minecraft:smite": 2}));
        let zombie = EntityFacts { type_id: "minecraft:zombie".into(), ..EntityFacts::default() };
        let cow = EntityFacts { type_id: "minecraft:cow".into(), ..EntityFacts::default() };
        let source = DamageFacts { damage_type: "minecraft:player_attack", attacker: None, direct: None, is_direct: true };
        assert_eq!(effects.damage(&sword, &cow, &source, 6.0, &mut random), 8.0);
        assert_eq!(effects.damage(&sword, &zombie, &source, 6.0, &mut random), 13.0);
        let boots = enchanted("minecraft:iron_boots", json!({"minecraft:feather_falling": 4, "minecraft:protection": 2}));
        let armor = [(Slot::Feet, &boots)];
        let fall = DamageFacts { damage_type: "minecraft:fall", ..source };
        assert_eq!(effects.damage_protection(&armor, &cow, &fall, &mut random), 14.0);
        assert_eq!(effects.damage_protection(&armor, &cow, &source, &mut random), 2.0);
        // Held in a hand, armor protects nothing.
        assert_eq!(effects.damage_protection(&[(Slot::MainHand, &boots)], &cow, &fall, &mut random), 0.0);
        assert!((damage_after_magic_absorb(10.0, 14.0) - 4.4).abs() < 1e-5);
    }

    #[test]
    fn unbreaking_spares_wear_and_efficiency_adds_its_modifier() {
        let effects = effects();
        let mut random = LegacyRandom::new(7);
        let pick = enchanted("minecraft:iron_pickaxe", json!({"minecraft:unbreaking": 3, "minecraft:efficiency": 3}));
        // Unbreaking III keeps three wear in four: of 400 single points,
        // about 100 land.
        let worn: i32 = (0..400).map(|_| effects.durability_change(&pick, 1, &mut random)).sum();
        assert!((60..140).contains(&worn), "{worn}");
        assert_eq!(effects.durability_change(&ItemStack::new("minecraft:iron_pickaxe", 1), 5, &mut random), 5);
        let modifiers = effects.attribute_modifiers(&pick, Slot::MainHand);
        assert_eq!(modifiers.len(), 1);
        assert_eq!((modifiers[0].attribute.as_str(), modifiers[0].id.as_str(), modifiers[0].amount), ("minecraft:mining_efficiency", "minecraft:enchantment.efficiency/mainhand", 10.0));
        assert!(effects.attribute_modifiers(&pick, Slot::OffHand).is_empty());
    }

    /// The game's own enchantments: thorns strikes back at random and
    /// wears its armor, bane of arthropods slows spiders, soul speed
    /// comes on over soul sand and goes off on leaving it.
    #[test]
    fn the_games_enchantments_act_when_available() {
        let Some(root) = std::env::var_os("MINECRAFTOSS_ROOT") else { return };
        let jar = std::path::Path::new(&root).join("client.jar");
        if !jar.exists() {
            return;
        }
        let book = crate::crafting::RecipeBook::from_jar(&jar).unwrap();
        let effects = book.enchantments();
        let mut random = LegacyRandom::new(3);
        let chest = enchanted("minecraft:iron_chestplate", json!({"minecraft:thorns": 3}));
        let zombie = EntityFacts { type_id: "minecraft:zombie".into(), ..EntityFacts::default() };
        let player = EntityFacts { type_id: "minecraft:player".into(), ..EntityFacts::default() };
        let source = DamageFacts { damage_type: "minecraft:mob_attack", attacker: Some(&zombie), direct: Some(&zombie), is_direct: true };
        let hits: Vec<_> = (0..200).map(|_| effects.post_attack(&[(Slot::Chest, &chest)], None, &player, &source, &mut random)).collect();
        let struck = hits.iter().filter(|h| !h.is_empty()).count();
        assert!((60..120).contains(&struck), "thorns III strikes 45% of the time: {struck}");
        let first = hits.iter().find(|h| !h.is_empty()).unwrap();
        assert!(matches!(&first[0], PostAttack { enchanted: Target::Victim, affected: Target::Attacker, slot: Slot::Chest, effect: EntityEffect::Damage { amount, .. } } if (1.0..5.0).contains(amount)));
        assert_eq!(first[1].effect, EntityEffect::ItemDamage { amount: 2 });
        let sword = enchanted("minecraft:iron_sword", json!({"minecraft:bane_of_arthropods": 1, "minecraft:fire_aspect": 2}));
        let spider = EntityFacts { type_id: "minecraft:spider".into(), ..EntityFacts::default() };
        let attack = DamageFacts { damage_type: "minecraft:player_attack", attacker: Some(&player), direct: Some(&player), is_direct: true };
        assert_eq!(effects.damage(&sword, &spider, &attack, 6.0, &mut random), 8.5);
        let after = effects.post_attack(&[], Some(&sword), &spider, &attack, &mut random);
        assert!(after.iter().any(|a| matches!(&a.effect, EntityEffect::MobEffect { effect, amplifier: 3, duration } if effect == "minecraft:slowness" && *duration == 30)), "{after:?}");
        assert!(after.iter().any(|a| a.effect == EntityEffect::Ignite { seconds: 8.0 }));
        let boots = enchanted("minecraft:iron_boots", json!({"minecraft:soul_speed": 3}));
        let equipment = [(Slot::Feet, &boots)];
        let mut state = LocationEffects::default();
        let on_sand = EntityFacts { on_ground: true, block_below: Some("minecraft:soul_sand".into()), ..player.clone() };
        let changes = effects.location_changed(&equipment, &on_sand, &mut state, &mut random);
        let speed = changes.iter().find_map(|c| match c {
            LocationChange::Add(m) if m.attribute == "minecraft:movement_speed" => Some(m.amount),
            _ => None,
        });
        assert!((speed.unwrap() - 0.0615).abs() < 1e-6, "{changes:?}");
        let on_grass = EntityFacts { block_below: Some("minecraft:grass_block".into()), ..on_sand };
        let changes = effects.location_changed(&equipment, &on_grass, &mut state, &mut random);
        assert_eq!(changes.iter().filter(|c| matches!(c, LocationChange::Remove(_))).count(), 2, "{changes:?}");
        assert!(state.is_empty());
        // Unbreaking on armor spares wear less often than on tools.
        let armor = enchanted("minecraft:iron_boots", json!({"minecraft:unbreaking": 3}));
        let worn: i32 = (0..1000).map(|_| effects.durability_change(&armor, 1, &mut random)).sum();
        assert!((650..750).contains(&worn), "{worn}");
    }

    #[test]
    fn the_older_levels_shape_is_read_too() {
        let stack = enchanted("minecraft:diamond_pickaxe", json!({"levels": {"minecraft:fortune": 3}}));
        assert_eq!(item_enchantments(&stack), [("minecraft:fortune", 3)]);
    }
}
