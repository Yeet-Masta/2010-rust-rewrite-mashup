//! Loot tables (26.3 `LootTable`, `LootPool`, the pool entries, the
//! condition and function types the vanilla data uses, and the int and float
//! context providers), read from the data pack: a block's drops, and a
//! container's loot (`LootTable.fill`).
//!
//! Tables with a `random_sequence` draw from that per-world named sequence
//! (`RandomSequences`), others from the caller's random. Constructs not
//! ported yet make the evaluation fail with a message instead of inventing
//! loot. Treasure maps (`exploration_map`) find no structure, as maps are
//! not simulated: the tables then discard them.

use crate::datapack::DataPack;
use crate::enchantment::{holder_set, Enchantments};
use crate::ident::Identifier;
use crate::item::ItemStack;
use crate::nbt::Tag;
use crate::random::{LegacyRandom, RandomSource, XoroshiroRandom};
use crate::{BlockStateId, Registries};
use serde_json::Value as Json;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

/// What a loot evaluation knows (`LootParams`).
#[derive(Clone, Debug, Default)]
pub struct LootParams {
    pub origin: Option<[f64; 3]>,
    /// The block broken or dropped (`BLOCK_STATE`).
    pub block_state: Option<BlockStateId>,
    /// The tool (`TOOL`); `Some(empty)` for blocks broken without one.
    pub tool: Option<ItemStack>,
    /// `THIS_ENTITY` is present.
    pub this_entity: bool,
    /// `EXPLOSION_RADIUS`.
    pub explosion_radius: Option<f32>,
    pub luck: f32,
    /// `BLOCK_ENTITY`, as the components it gives (`collectComponents`):
    /// saved component NBT by component id.
    pub block_entity: Option<crate::nbt::Tag>,
    /// The biome at `origin`, by id (`location_check`'s `biomes`).
    pub biome: Option<String>,
}

/// Parsed loot tables and predicates, loaded on first use.
pub struct LootTables {
    tables: Mutex<HashMap<String, Option<Json>>>,
    predicates: Mutex<HashMap<String, Option<Json>>>,
    /// The enchantment registry the enchanting functions choose from.
    enchantments: OnceLock<Result<Enchantments>>,
}

impl Default for LootTables {
    fn default() -> Self {
        Self { tables: Mutex::new(HashMap::new()), predicates: Mutex::new(HashMap::new()), enchantments: OnceLock::new() }
    }
}

type Result<T> = std::result::Result<T, String>;

struct Context<'a> {
    registries: &'a Registries,
    tables: &'a LootTables,
    params: &'a LootParams,
    random: &'a mut dyn RandomSource,
    /// Tables being evaluated (`LootContext.visitedElements`).
    visiting: Vec<String>,
}

impl XoroshiroRandom {
    /// `RandomSequence(seed, key)`: the world seed, unmixed, xored with the
    /// key's MD5, then mixed.
    pub fn for_sequence(world_seed: i64, key: &str) -> Self {
        const SILVER: u64 = 0x6a09_e667_f3bc_c909;
        const GOLDEN: u64 = 0x9e37_79b9_7f4a_7c15;
        let lo = (world_seed as u64) ^ SILVER;
        let hi = lo.wrapping_add(GOLDEN);
        let digest = md5::compute(key.as_bytes());
        let hash_lo = u64::from_be_bytes(digest[0..8].try_into().expect("MD5 half"));
        let hash_hi = u64::from_be_bytes(digest[8..16].try_into().expect("MD5 half"));
        let mix = |mut z: u64| {
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        };
        XoroshiroRandom::from_state(mix(lo ^ hash_lo), mix(hi ^ hash_hi))
    }
}

/// Per-world named random sequences (`RandomSequences`).
#[derive(Debug, Default)]
pub struct RandomSequences {
    pub world_seed: i64,
    sequences: HashMap<String, XoroshiroRandom>,
}

impl RandomSequences {
    pub fn new(world_seed: i64) -> Self {
        Self { world_seed, sequences: HashMap::new() }
    }

    pub fn get(&mut self, key: &str) -> &mut XoroshiroRandom {
        let seed = self.world_seed;
        self.sequences.entry(key.to_owned()).or_insert_with(|| XoroshiroRandom::for_sequence(seed, key))
    }
}

/// A chain with one more modifier inside it.
fn with_inner(modifier: Option<&Json>, chain: &[Json]) -> Vec<Json> {
    let mut out: Vec<Json> = modifier.cloned().into_iter().collect();
    out.extend(chain.iter().cloned());
    out
}

fn id_of(text: &str) -> String {
    if text.contains(':') { text.to_owned() } else { format!("minecraft:{text}") }
}

impl LootTables {
    fn load(map: &Mutex<HashMap<String, Option<Json>>>, pack: &DataPack, kind: &str, id: &str) -> Option<Json> {
        let mut map = map.lock().expect("loot cache");
        map.entry(id.to_owned())
            .or_insert_with(|| Identifier::parse(id).ok().and_then(|ident| pack.read_json(kind, &ident).ok()))
            .clone()
    }

    fn table(&self, registries: &Registries, id: &str) -> Option<Json> {
        Self::load(&self.tables, &registries.datapack, "loot_table", id)
    }

    fn predicate(&self, registries: &Registries, id: &str) -> Option<Json> {
        Self::load(&self.predicates, &registries.datapack, "predicate", id)
    }

    /// The enchantment registry, read on first use.
    fn enchantments(&self, registries: &Registries) -> Result<&Enchantments> {
        self.enchantments.get_or_init(|| Enchantments::load(&registries.datapack)).as_ref().map_err(Clone::clone)
    }

    /// `LootTable.getRandomItems(params)`: the table's own sequence when it
    /// names one, else `random`, with stacks split to their size.
    pub fn roll(
        &self,
        registries: &Registries,
        id: &str,
        params: &LootParams,
        sequences: &mut RandomSequences,
        random: &mut dyn RandomSource,
    ) -> Result<Vec<ItemStack>> {
        let Some(table) = self.table(registries, id) else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        match table.get("random_sequence").and_then(Json::as_str) {
            Some(sequence) => {
                let random = sequences.get(sequence);
                let mut context = Context { registries, tables: self, params, random, visiting: Vec::new() };
                context.table_items(id, &table, &[], &mut out)?;
            }
            None => {
                let mut context = Context { registries, tables: self, params, random, visiting: Vec::new() };
                context.table_items(id, &table, &[], &mut out)?;
            }
        }
        Ok(split_stacks(registries, out))
    }

    /// `LootTable.fill`: the table rolled for a container whose empty
    /// slots are `empty`, in slot order. The random comes from `seed`
    /// (`withOptionalRandomSeed`, `RandomSource.create`), or for a seed of
    /// 0 from the table's sequence, else `random`. The empty slots are
    /// shuffled, the stacks split further while there are more slots than
    /// stacks (`shuffleAndSplitItems`), and each stack in turn takes the
    /// last of the shuffled slots left; what finds none is lost ("Tried to
    /// over-fill a container"). Returns the (slot, stack)s placed, in
    /// order. A missing table fills nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn fill(
        &self,
        registries: &Registries,
        id: &str,
        params: &LootParams,
        seed: i64,
        sequences: &mut RandomSequences,
        random: &mut dyn RandomSource,
        empty: &[usize],
    ) -> Result<Vec<(usize, ItemStack)>> {
        let Some(table) = self.table(registries, id) else { return Ok(Vec::new()) };
        let mut seeded = LegacyRandom::new(seed);
        let random: &mut dyn RandomSource = match table.get("random_sequence").and_then(Json::as_str) {
            _ if seed != 0 => &mut seeded,
            Some(sequence) => sequences.get(sequence),
            None => random,
        };
        let mut context = Context { registries, tables: self, params, random, visiting: Vec::new() };
        let mut rolled = Vec::new();
        context.table_items(id, &table, &[], &mut rolled)?;
        let mut stacks = split_stacks(registries, rolled);
        // `getAvailableSlots`.
        let mut slots = empty.to_vec();
        shuffle(&mut slots, context.random);
        shuffle_and_split(&mut stacks, slots.len(), context.random);
        let mut placed = Vec::new();
        for stack in stacks {
            let Some(slot) = slots.pop() else { break };
            placed.push((slot, stack));
        }
        Ok(placed)
    }

    /// A block's own loot table (`blocks/<name>`).
    pub fn block_drops(
        &self,
        registries: &Registries,
        state: BlockStateId,
        params: &LootParams,
        sequences: &mut RandomSequences,
        random: &mut dyn RandomSource,
    ) -> Result<Vec<ItemStack>> {
        let blocks = &registries.blocks;
        let name = blocks.block(blocks.block_of(state)).name.as_str().to_owned();
        let (namespace, path) = name.split_once(':').unwrap_or(("minecraft", &name));
        let mut params = params.clone();
        params.block_state = Some(state);
        self.roll(registries, &format!("{namespace}:blocks/{path}"), &params, sequences, random)
    }
}

/// `LootTable.createStackSplitter`: a stack of its item's size or more in
/// stacks of that size.
fn split_stacks(registries: &Registries, stacks: Vec<ItemStack>) -> Vec<ItemStack> {
    let mut split = Vec::new();
    for stack in stacks {
        let max = registries.items.max_stack(&stack.id);
        if stack.count < max {
            split.push(stack);
        } else {
            let mut count = stack.count;
            while count > 0 {
                let mut part = stack.clone();
                part.count = count.min(max);
                count -= part.count;
                split.push(part);
            }
        }
    }
    split
}

/// `Util.shuffle`: from the last place down to the second, each swapped
/// with a place at or before it.
fn shuffle<T>(list: &mut [T], random: &mut dyn RandomSource) {
    for size in (2..=list.len()).rev() {
        let other = random.next_i32_bound(size as i32) as usize;
        list.swap(size - 1, other);
    }
}

/// `Mth.nextInt(random, min, max)`: inclusive, and no draw when the range
/// is one number.
fn next_between(random: &mut dyn RandomSource, min: i32, max: i32) -> i32 {
    if min >= max { min } else { random.next_i32_bound(max - min + 1) + min }
}

/// `LootTable.shuffleAndSplitItems`: empty stacks go, and stacks of several
/// are set aside to split. While the stacks are fewer than the free slots,
/// one set aside (drawn at random) gives up a part of at most half of it,
/// and each of the two is set aside again on a coin toss if it is still
/// several. Then all of them, those set aside last, are shuffled.
fn shuffle_and_split(stacks: &mut Vec<ItemStack>, slots: usize, random: &mut dyn RandomSource) {
    let mut splittable = Vec::new();
    let mut kept = Vec::new();
    for stack in stacks.drain(..) {
        if stack.is_empty() {
            continue;
        }
        if stack.count > 1 {
            splittable.push(stack);
        } else {
            kept.push(stack);
        }
    }
    while slots > kept.len() + splittable.len() && !splittable.is_empty() {
        let index = next_between(random, 0, splittable.len() as i32 - 1) as usize;
        let mut stack = splittable.remove(index);
        let mut part = stack.clone();
        part.count = next_between(random, 1, stack.count / 2);
        stack.count -= part.count;
        for piece in [stack, part] {
            if piece.count > 1 && random.next_bool() {
                splittable.push(piece);
            } else {
                kept.push(piece);
            }
        }
    }
    kept.append(&mut splittable);
    shuffle(&mut kept, random);
    *stacks = kept;
}

/// The component `id` of a stack's patch.
fn component<'s>(stack: &'s ItemStack, id: &str) -> Option<&'s Tag> {
    stack.components.as_ref()?.get(id)
}

/// Sets (or, for `None`, takes out) a component of a stack's patch; an
/// empty patch is none.
fn set_component(stack: &mut ItemStack, id: &str, value: Option<Tag>) {
    let mut patch = match stack.components.take() {
        Some(Tag::Compound(map)) => map,
        _ => BTreeMap::new(),
    };
    match value {
        Some(value) => patch.insert(id.to_owned(), value),
        None => patch.remove(id),
    };
    stack.components = (!patch.is_empty()).then_some(Tag::Compound(patch));
}

/// `EnchantmentHelper.getComponentType`: an enchanted book stores its
/// enchantments, anything else has them.
fn enchantments_component(stack: &ItemStack) -> &'static str {
    if stack.id == "minecraft:enchanted_book" { "minecraft:stored_enchantments" } else { "minecraft:enchantments" }
}

fn enchantment_level(stack: &ItemStack, id: &str) -> i32 {
    component(stack, enchantments_component(stack)).and_then(|e| e.get(id)).and_then(Tag::as_i64).unwrap_or(0) as i32
}

/// `ItemEnchantments.Mutable.set`: at most 255, and 0 or less removes it.
/// No enchantments left is the item's default, no component.
fn set_enchantment(stack: &mut ItemStack, id: &str, level: i32) {
    let key = enchantments_component(stack);
    let mut map = match component(stack, key) {
        Some(Tag::Compound(map)) => map.clone(),
        _ => BTreeMap::new(),
    };
    if level <= 0 {
        map.remove(id);
    } else {
        map.insert(id.to_owned(), Tag::Int(level.min(255)));
    }
    set_component(stack, key, (!map.is_empty()).then_some(Tag::Compound(map)));
}

/// `ItemStack.enchant` (`ItemEnchantments.Mutable.upgrade`): the level
/// raised to `level`, never lowered.
fn upgrade_enchantment(stack: &mut ItemStack, id: &str, level: i32) {
    if level > 0 {
        let now = enchantment_level(stack, id);
        set_enchantment(stack, id, now.max(level.min(255)));
    }
}

/// `MobEffect.isInstantaneous`: the instant health and damage effects, and
/// saturation.
fn instantaneous(effect: &str) -> bool {
    matches!(effect, "minecraft:instant_health" | "minecraft:instant_damage" | "minecraft:saturation")
}

/// A text component's JSON as NBT (`ComponentSerialization` through NBT):
/// booleans as bytes, whole numbers as ints, objects and lists as they are.
fn text_nbt(value: &Json) -> Tag {
    match value {
        Json::String(text) => Tag::String(text.clone()),
        Json::Bool(flag) => Tag::Byte(i8::from(*flag)),
        Json::Number(n) => match n.as_i64() {
            Some(whole) => Tag::Int(whole as i32),
            None => Tag::Double(n.as_f64().unwrap_or(0.0)),
        },
        Json::Array(list) => Tag::List(list.iter().map(text_nbt).collect()),
        Json::Object(map) => Tag::Compound(map.iter().map(|(key, value)| (key.clone(), text_nbt(value))).collect()),
        Json::Null => Tag::String(String::new()),
    }
}

impl Context<'_> {
    /// `LootTable.getRandomItemsRaw`. `chain` holds the modifiers that
    /// decorate the output, innermost first; each stack runs through them as
    /// soon as it is made, as vanilla's decorated consumers do.
    fn table_items(&mut self, id: &str, table: &Json, chain: &[Json], out: &mut Vec<ItemStack>) -> Result<()> {
        if self.visiting.iter().any(|v| v == id) {
            return Ok(());
        }
        self.visiting.push(id.to_owned());
        let chain = with_inner(table.get("modifier"), chain);
        for pool in table.get("pools").and_then(Json::as_array).into_iter().flatten() {
            self.pool_items(pool, &chain, out)?;
        }
        self.visiting.pop();
        Ok(())
    }

    /// Runs a new stack through a modifier chain into the output.
    fn emit(&mut self, mut stack: ItemStack, chain: &[Json], out: &mut Vec<ItemStack>) -> Result<()> {
        for modifier in chain {
            stack = self.apply_modifier(Some(modifier), stack)?;
        }
        out.push(stack);
        Ok(())
    }

    /// `LootPool.addRandomItems`.
    fn pool_items(&mut self, pool: &Json, chain: &[Json], out: &mut Vec<ItemStack>) -> Result<()> {
        if !self.condition_opt(pool.get("condition"))? {
            return Ok(());
        }
        let rolls = self.int(pool.get("rolls").ok_or("pool without rolls")?)?;
        let bonus = match pool.get("bonus_rolls") {
            Some(b) => self.float(b)?,
            None => 0.0,
        };
        let count = rolls + (bonus * self.params.luck).floor() as i32;
        let chain = with_inner(pool.get("modifier"), chain);
        for _ in 0..count {
            self.add_random_item(pool, &chain, out)?;
        }
        Ok(())
    }

    /// `LootPool.addRandomItem`: expand the entries, then pick by weight.
    fn add_random_item(&mut self, pool: &Json, chain: &[Json], out: &mut Vec<ItemStack>) -> Result<()> {
        let mut valid: Vec<(Json, Vec<Json>, i32)> = Vec::new();
        for entry in pool.get("entries").and_then(Json::as_array).into_iter().flatten() {
            let mut expanded = Vec::new();
            self.expand(entry, &mut Vec::new(), &mut expanded)?;
            for (leaf, modifiers) in expanded {
                let weight = self.weight(&leaf);
                if weight > 0 {
                    valid.push((leaf, modifiers, weight));
                }
            }
        }
        let total: i32 = valid.iter().map(|v| v.2).sum();
        if total == 0 || valid.is_empty() {
            return Ok(());
        }
        let chosen = if valid.len() == 1 {
            0
        } else {
            let mut index = self.random.next_i32_bound(total);
            let mut chosen = valid.len() - 1;
            for (i, v) in valid.iter().enumerate() {
                index -= v.2;
                if index < 0 {
                    chosen = i;
                    break;
                }
            }
            chosen
        };
        let (leaf, modifiers, _) = valid.swap_remove(chosen);
        // The leaf's modifier, then its enclosing composites' from the
        // innermost out, then the pool's and the table's.
        let mut full: Vec<Json> = leaf.get("modifier").cloned().into_iter().collect();
        full.extend(modifiers.iter().rev().cloned());
        full.extend(chain.iter().cloned());
        self.create_items(&leaf, &full, out)
    }

    fn weight(&self, leaf: &Json) -> i32 {
        let weight = leaf.get("weight").and_then(Json::as_i64).unwrap_or(1) as f32;
        let quality = leaf.get("quality").and_then(Json::as_i64).unwrap_or(0) as f32;
        ((weight + quality * self.params.luck).floor() as i32).max(0)
    }

    /// `LootPoolEntryContainer.expand`: a condition gate, then the entry's
    /// own expansion; composite entries' modifiers wrap their children.
    fn expand(&mut self, entry: &Json, modifiers: &mut Vec<Json>, out: &mut Vec<(Json, Vec<Json>)>) -> Result<bool> {
        if !self.condition_opt(entry.get("condition"))? {
            return Ok(false);
        }
        let kind = entry.get("type").and_then(Json::as_str).unwrap_or("minecraft:item");
        match kind.trim_start_matches("minecraft:") {
            "item" | "empty" | "loot_table" | "dynamic" => {
                out.push((entry.clone(), modifiers.clone()));
                Ok(true)
            }
            "alternatives" | "group" | "sequence" => {
                let children: Vec<Json> = entry.get("children").and_then(Json::as_array).cloned().unwrap_or_default();
                let pushed = entry.get("modifier").cloned();
                if let Some(m) = &pushed {
                    modifiers.push(m.clone());
                }
                let result = match kind.trim_start_matches("minecraft:") {
                    "alternatives" => {
                        let mut any = false;
                        for child in &children {
                            if self.expand(child, modifiers, out)? {
                                any = true;
                                break;
                            }
                        }
                        any
                    }
                    "group" => {
                        for child in &children {
                            self.expand(child, modifiers, out)?;
                        }
                        true
                    }
                    _ => {
                        let mut all = true;
                        for child in &children {
                            if !self.expand(child, modifiers, out)? {
                                all = false;
                                break;
                            }
                        }
                        all
                    }
                };
                if pushed.is_some() {
                    modifiers.pop();
                }
                Ok(result)
            }
            other => Err(format!("loot entry type {other} is not supported")),
        }
    }

    /// `createItemStack` of a leaf entry.
    fn create_items(&mut self, leaf: &Json, chain: &[Json], out: &mut Vec<ItemStack>) -> Result<()> {
        let kind = leaf.get("type").and_then(Json::as_str).unwrap_or("minecraft:item");
        match kind.trim_start_matches("minecraft:") {
            "item" => {
                let name = leaf.get("name").and_then(Json::as_str).ok_or("item entry without name")?;
                self.emit(ItemStack::new(&id_of(name), 1), chain, out)
            }
            "empty" => Ok(()),
            "loot_table" => {
                let value = leaf.get("value").ok_or("loot_table entry without value")?;
                match value {
                    Json::String(id) => {
                        let id = id_of(id);
                        let table = self.tables.table(self.registries, &id).ok_or_else(|| format!("missing loot table {id}"))?;
                        self.table_items(&id, &table, chain, out)
                    }
                    inline => self.table_items("<inline>", &inline.clone(), chain, out),
                }
            }
            other => Err(format!("loot entry type {other} is not supported")),
        }
    }

    // ---- providers -------------------------------------------------------------

    /// A `ContextIntProvider`.
    fn int(&mut self, value: &Json) -> Result<i32> {
        if let Some(n) = value.as_f64() {
            return Ok(n as i32);
        }
        let kind = value.get("type").and_then(Json::as_str).unwrap_or("minecraft:constant");
        match kind.trim_start_matches("minecraft:") {
            "constant" => Ok(value.get("value").and_then(Json::as_f64).unwrap_or(0.0) as i32),
            "uniform" => {
                let min = self.int(value.get("min").ok_or("uniform without min")?)?;
                let max = self.int(value.get("max").ok_or("uniform without max")?)?;
                Ok(if min >= max { min } else { self.random.next_i32_bound(max - min + 1) + min })
            }
            "binomial" => {
                let n = self.int(value.get("n").ok_or("binomial without n")?)?;
                let p = self.float(value.get("p").ok_or("binomial without p")?)?;
                let mut result = 0;
                for _ in 0..n {
                    if self.random.next_f32() < p {
                        result += 1;
                    }
                }
                Ok(result)
            }
            other => Err(format!("int provider {other} is not supported")),
        }
    }

    /// A `ContextFloatProvider`.
    fn float(&mut self, value: &Json) -> Result<f32> {
        if let Some(n) = value.as_f64() {
            return Ok(n as f32);
        }
        let kind = value.get("type").and_then(Json::as_str).unwrap_or("minecraft:constant");
        match kind.trim_start_matches("minecraft:") {
            "constant" => Ok(value.get("value").and_then(Json::as_f64).unwrap_or(0.0) as f32),
            "uniform" => {
                let min = self.float(value.get("min").ok_or("uniform without min")?)?;
                let max = self.float(value.get("max").ok_or("uniform without max")?)?;
                Ok(if min >= max { min } else { self.random.next_f32() * (max - min) + min })
            }
            other => Err(format!("float provider {other} is not supported")),
        }
    }

    // ---- conditions ------------------------------------------------------------

    fn condition_opt(&mut self, condition: Option<&Json>) -> Result<bool> {
        match condition {
            None => Ok(true),
            Some(c) => self.condition(c),
        }
    }

    /// `LootItemCondition.test`.
    fn condition(&mut self, condition: &Json) -> Result<bool> {
        if let Some(reference) = condition.as_str() {
            let id = id_of(reference);
            let predicate = self.tables.predicate(self.registries, &id).ok_or_else(|| format!("missing predicate {id}"))?;
            return self.condition(&predicate);
        }
        let kind = condition.get("type").and_then(Json::as_str).ok_or("condition without type")?;
        match kind.trim_start_matches("minecraft:") {
            "survives_explosion" => match self.params.explosion_radius {
                Some(radius) => Ok(self.random.next_f32() <= 1.0 / radius),
                None => Ok(true),
            },
            "inverted" => Ok(!self.condition(condition.get("term").ok_or("inverted without term")?)?),
            "any_of" => {
                for term in condition.get("terms").and_then(Json::as_array).into_iter().flatten() {
                    if self.condition(term)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            "all_of" => {
                for term in condition.get("terms").and_then(Json::as_array).into_iter().flatten() {
                    if !self.condition(term)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            "random_chance" => {
                let chance = self.float(condition.get("chance").ok_or("random_chance without chance")?)?;
                Ok(self.random.next_f32() < chance)
            }
            "table_bonus" => {
                let enchantment = condition.get("enchantment").and_then(Json::as_str).unwrap_or_default();
                let level = self.tool_enchantment(enchantment);
                let chances: Vec<f32> = condition.get("chances").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_f64).map(|c| c as f32).collect();
                let chance = chances.get((level as usize).min(chances.len().saturating_sub(1))).copied().unwrap_or(0.0);
                Ok(self.random.next_f32() < chance)
            }
            "match_block" => Ok(self.match_block(condition)),
            // `LocationCheck` at the origin, for its biomes.
            "location_check" => {
                if ["offsetX", "offsetY", "offsetZ"].iter().any(|axis| condition.get(*axis).and_then(Json::as_i64).is_some_and(|offset| offset != 0)) {
                    return Err("location_check with an offset is not supported".to_owned());
                }
                if self.params.origin.is_none() {
                    return Ok(false);
                }
                let Some(predicate) = condition.get("predicate").and_then(Json::as_object) else { return Ok(true) };
                if let Some(other) = predicate.keys().find(|key| *key != "biomes") {
                    return Err(format!("location predicate {other} is not supported"));
                }
                let Some(biomes) = predicate.get("biomes") else { return Ok(true) };
                let biomes = holder_set(&self.registries.datapack, "worldgen/biome", biomes);
                Ok(self.params.biome.as_ref().is_some_and(|biome| biomes.contains(biome)))
            }
            "match_tool" => {
                let Some(tool) = &self.params.tool else { return Ok(false) };
                match condition.get("predicate") {
                    None => Ok(true),
                    Some(predicate) => self.item_predicate(tool, predicate),
                }
            }
            "entity_properties" => {
                if !self.params.this_entity {
                    return Ok(false);
                }
                match condition.get("predicate") {
                    Some(Json::Object(map)) if map.is_empty() => Ok(true),
                    None => Ok(true),
                    _ => Err("entity property predicates are not supported".to_owned()),
                }
            }
            other => Err(format!("loot condition {other} is not supported")),
        }
    }

    /// `MatchBlock`: the block (id, list or tag) and state properties.
    fn match_block(&self, condition: &Json) -> bool {
        let Some(state) = self.params.block_state else { return false };
        let blocks = &self.registries.blocks;
        let name = blocks.block(blocks.block_of(state)).name.as_str();
        let block_ok = match condition.get("blocks") {
            None => true,
            Some(Json::String(s)) if s.starts_with('#') => {
                let tag = id_of(&s[1..]);
                self.registries.block_tags.id(&tag).is_some_and(|t| self.registries.block_in_tag(state, t))
            }
            Some(Json::String(s)) => id_of(s) == name,
            Some(Json::Array(list)) => list.iter().filter_map(Json::as_str).any(|s| id_of(s) == name),
            _ => false,
        };
        if !block_ok {
            return false;
        }
        for (key, want) in condition.get("state").and_then(Json::as_object).into_iter().flatten() {
            let Some(have) = blocks.property(state, key) else { return false };
            let ok = match want {
                Json::String(v) => have == v,
                Json::Bool(b) => have == if *b { "true" } else { "false" },
                Json::Number(n) => have == n.to_string(),
                Json::Object(range) => {
                    let value: i64 = match have.parse() {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    range.get("min").and_then(Json::as_i64).is_none_or(|m| value >= m) && range.get("max").and_then(Json::as_i64).is_none_or(|m| value <= m)
                }
                _ => false,
            };
            if !ok {
                return false;
            }
        }
        true
    }

    /// `ItemPredicate.test` for the parts vanilla loot uses.
    fn item_predicate(&self, stack: &ItemStack, predicate: &Json) -> Result<bool> {
        if stack.is_empty() && (predicate.get("items").is_some() || predicate.get("predicates").is_some()) {
            return Ok(false);
        }
        if let Some(items) = predicate.get("items") {
            let ok = match items {
                Json::String(s) if s.starts_with('#') => return Err("item tag predicates are not supported".to_owned()),
                Json::String(s) => id_of(s) == stack.id,
                Json::Array(list) => list.iter().filter_map(Json::as_str).any(|s| id_of(s) == stack.id),
                _ => false,
            };
            if !ok {
                return Ok(false);
            }
        }
        if let Some(predicates) = predicate.get("predicates").and_then(Json::as_object) {
            for (key, value) in predicates {
                match key.as_str() {
                    "minecraft:enchantments" => {
                        for wanted in value.as_array().into_iter().flatten() {
                            let enchantment = wanted.get("enchantments").and_then(Json::as_str).unwrap_or_default();
                            let level = self.tool_enchantment(enchantment);
                            let min = wanted.get("levels").and_then(|l| l.get("min")).and_then(Json::as_i64).unwrap_or(1);
                            if (level as i64) < min {
                                return Ok(false);
                            }
                        }
                    }
                    // Any other component as `{}` (`AnyValue`): the stack has it.
                    other if value.as_object().is_some_and(serde_json::Map::is_empty) => {
                        if component(stack, other).is_none() {
                            return Ok(false);
                        }
                    }
                    other => return Err(format!("item sub-predicate {other} is not supported")),
                }
            }
        }
        Ok(true)
    }

    /// The tool's level of an enchantment (`minecraft:enchantments` component).
    fn tool_enchantment(&self, enchantment: &str) -> i32 {
        let Some(tool) = &self.params.tool else { return 0 };
        let id = id_of(enchantment);
        tool.components
            .as_ref()
            .and_then(|c| c.get("minecraft:enchantments"))
            .and_then(|e| e.get(&id))
            .and_then(crate::nbt::Tag::as_i64)
            .unwrap_or(0) as i32
    }

    // ---- functions ---------------------------------------------------------------

    /// `LootItemFunction.apply` for a `modifier` (one function or a list).
    fn apply_modifier(&mut self, modifier: Option<&Json>, mut stack: ItemStack) -> Result<ItemStack> {
        match modifier {
            None => Ok(stack),
            Some(Json::Array(list)) => {
                for function in list {
                    stack = self.apply_function(function, stack)?;
                }
                Ok(stack)
            }
            Some(function) => self.apply_function(function, stack),
        }
    }

    fn apply_function(&mut self, function: &Json, mut stack: ItemStack) -> Result<ItemStack> {
        if let Some(condition) = function.get("condition") {
            if !self.condition(condition)? {
                return Ok(stack);
            }
        }
        let kind = function.get("type").and_then(Json::as_str).ok_or("function without type")?;
        match kind.trim_start_matches("minecraft:") {
            "set_count" => {
                let add = function.get("add").and_then(Json::as_bool).unwrap_or(false);
                let base = if add { stack.count } else { 0 };
                stack.count = base + self.int(function.get("count").ok_or("set_count without count")?)?;
            }
            "explosion_decay" => {
                if let Some(radius) = self.params.explosion_radius {
                    let probability = 1.0 / radius;
                    let mut result = 0;
                    for _ in 0..stack.count {
                        if self.random.next_f32() <= probability {
                            result += 1;
                        }
                    }
                    stack.count = result;
                }
            }
            "apply_bonus" => {
                if self.params.tool.is_some() {
                    let enchantment = function.get("enchantment").and_then(Json::as_str).unwrap_or_default();
                    let level = self.tool_enchantment(enchantment);
                    let formula = function.get("formula").and_then(Json::as_str).unwrap_or_default();
                    stack.count = match formula.trim_start_matches("minecraft:") {
                        "ore_drops" => {
                            if level > 0 {
                                let bonus = (self.random.next_i32_bound(level + 2) - 1).max(0);
                                stack.count * (bonus + 1)
                            } else {
                                stack.count
                            }
                        }
                        "uniform_bonus_count" => {
                            let multiplier = function.get("parameters").and_then(|p| p.get("bonusMultiplier")).and_then(Json::as_i64).unwrap_or(1) as i32;
                            stack.count + self.random.next_i32_bound(multiplier * level + 1)
                        }
                        "binomial_with_bonus_count" => {
                            let parameters = function.get("parameters");
                            let extra = parameters.and_then(|p| p.get("extra")).and_then(Json::as_i64).unwrap_or(0) as i32;
                            let probability = parameters.and_then(|p| p.get("probability")).and_then(Json::as_f64).unwrap_or(0.0) as f32;
                            let mut count = stack.count;
                            for _ in 0..level + extra {
                                if self.random.next_f32() < probability {
                                    count += 1;
                                }
                            }
                            count
                        }
                        other => return Err(format!("bonus formula {other} is not supported")),
                    };
                }
            }
            "limit_count" => {
                let limit = function.get("limit").ok_or("limit_count without limit")?;
                let (min, max) = match limit {
                    Json::Number(n) => (n.as_i64(), n.as_i64()),
                    other => (other.get("min").and_then(Json::as_i64), other.get("max").and_then(Json::as_i64)),
                };
                if let Some(min) = min {
                    stack.count = stack.count.max(min as i32);
                }
                if let Some(max) = max {
                    stack.count = stack.count.min(max as i32);
                }
            }
            // `CopyComponentsFunction` from the block entity broken: the
            // included components it has, less the excluded ones. Other
            // sources, and block states, are cosmetic for drops without them.
            "copy_components" | "copy_state" => {
                if self.params.block_state.is_none() {
                    return Err(format!("{kind} without a block"));
                }
                let from_block_entity = function.get("source").and_then(Json::as_str) == Some("block_entity");
                if let (true, Some(crate::nbt::Tag::Compound(source))) = (kind.ends_with("copy_components") && from_block_entity, &self.params.block_entity) {
                    let names = |field: &str| function.get(field).and_then(Json::as_array).map(|ids| ids.iter().filter_map(Json::as_str).map(id_of).collect::<Vec<_>>());
                    let (include, exclude) = (names("include"), names("exclude"));
                    for (key, value) in source {
                        let id = id_of(key);
                        if include.as_ref().is_some_and(|ids| !ids.contains(&id)) || exclude.as_ref().is_some_and(|ids| ids.contains(&id)) {
                            continue;
                        }
                        let mut patch = match stack.components.take() {
                            Some(crate::nbt::Tag::Compound(map)) => map,
                            _ => Default::default(),
                        };
                        patch.insert(key.clone(), value.clone());
                        stack.components = Some(crate::nbt::Tag::Compound(patch));
                    }
                }
            }
            // `EnchantRandomlyFunction`: one of the options (every
            // enchantment for none) that can go on the item unless it is a
            // book, at a random level; a book becomes an enchanted book. With
            // none that fits the stack is unchanged. The trade cost it may
            // add needs a trade's context.
            "enchant_randomly" => {
                let enchantments = self.tables.enchantments(self.registries)?;
                let book = stack.id == "minecraft:book";
                let check = !book && function.get("only_compatible").and_then(Json::as_bool).unwrap_or(true);
                let candidates: Vec<usize> = enchantments.source(function.get("options")).into_iter().filter(|&index| !check || enchantments.list[index].can_enchant(&stack.id)).collect();
                if !candidates.is_empty() {
                    let chosen = &enchantments.list[candidates[self.random.next_i32_bound(candidates.len() as i32) as usize]];
                    let level = next_between(self.random, 1, chosen.max_level);
                    if book {
                        stack = ItemStack::new("minecraft:enchanted_book", 1);
                    }
                    upgrade_enchantment(&mut stack, &chosen.id, level);
                }
            }
            // `EnchantWithLevelsFunction` (`EnchantmentHelper.enchantItem`):
            // enchantments chosen for the cost, a book becoming an enchanted
            // book whatever they are.
            "enchant_with_levels" => {
                let cost = self.int(function.get("levels").ok_or("enchant_with_levels without levels")?)?;
                let enchantments = self.tables.enchantments(self.registries)?;
                let source = enchantments.source(function.get("options"));
                let chosen = enchantments.select(self.random, &stack.id, self.registries.items.enchantable(&stack.id), cost, &source);
                if stack.id == "minecraft:book" {
                    stack = ItemStack::new("minecraft:enchanted_book", 1);
                }
                for (index, level) in chosen {
                    upgrade_enchantment(&mut stack, &enchantments.list[index].id, level);
                }
            }
            // `SetEnchantmentsFunction`: each level set (or added to), a book
            // turned into an enchanted book as it is.
            "set_enchantments" => {
                if stack.id == "minecraft:book" {
                    stack.id = "minecraft:enchanted_book".to_owned();
                }
                let add = function.get("add").and_then(Json::as_bool).unwrap_or(false);
                for (id, level) in function.get("enchantments").and_then(Json::as_object).into_iter().flatten() {
                    let id = id_of(id);
                    let value = self.int(level)?;
                    let level = if add { enchantment_level(&stack, &id) + value } else { value };
                    set_enchantment(&mut stack, &id, level.clamp(0, 255));
                }
            }
            // `SetItemDamageFunction`, for an item that wears: the damage
            // that leaves the drawn fraction of its durability (added to
            // what it has, with `add`). No damage is the item's default.
            "set_damage" => {
                let unbreakable = component(&stack, "minecraft:unbreakable").is_some();
                if let Some(max) = self.registries.items.max_damage(&stack.id).filter(|&max| max > 0 && !unbreakable) {
                    let damage = component(&stack, "minecraft:damage").and_then(Tag::as_i64).unwrap_or(0) as f32;
                    let base = if function.get("add").and_then(Json::as_bool).unwrap_or(false) { 1.0 - damage / max as f32 } else { 0.0 };
                    let left = 1.0 - (self.float(function.get("damage").ok_or("set_damage without damage")?)? + base).clamp(0.0, 1.0);
                    let damage = ((left * max as f32).floor() as i32).clamp(0, max);
                    set_component(&mut stack, "minecraft:damage", (damage != 0).then_some(Tag::Int(damage)));
                }
            }
            // `SetNameFunction` with a fixed name, as the item's name or its
            // custom name.
            "set_name" => {
                if let Some(name) = function.get("name") {
                    let target = match function.get("target").and_then(Json::as_str) {
                        Some("item_name") => "minecraft:item_name",
                        _ => "minecraft:custom_name",
                    };
                    set_component(&mut stack, target, Some(text_nbt(name)));
                }
            }
            // `SetPotionFunction` (`PotionContents.withPotion`).
            "set_potion" => {
                let potion = function.get("id").and_then(Json::as_str).ok_or("set_potion without id")?;
                let mut contents = match component(&stack, "minecraft:potion_contents") {
                    Some(Tag::Compound(map)) => map.clone(),
                    _ => BTreeMap::new(),
                };
                contents.insert("potion".to_owned(), Tag::String(id_of(potion)));
                set_component(&mut stack, "minecraft:potion_contents", Some(Tag::Compound(contents)));
            }
            // `SetStewEffectFunction`: one of the effects, its duration in
            // seconds made ticks unless the effect is instant, added to a
            // suspicious stew's (the default 160 left out).
            "set_stew_effect" => {
                let effects = function.get("effects").and_then(Json::as_array).cloned().unwrap_or_default();
                if stack.id == "minecraft:suspicious_stew" && !effects.is_empty() {
                    let entry = &effects[self.random.next_i32_bound(effects.len() as i32) as usize];
                    let effect = id_of(entry.get("type").and_then(Json::as_str).ok_or("stew effect without type")?);
                    let mut duration = self.int(entry.get("duration").ok_or("stew effect without duration")?)?;
                    if !instantaneous(&effect) {
                        duration *= 20;
                    }
                    let mut list = match component(&stack, "minecraft:suspicious_stew_effects") {
                        Some(Tag::List(list)) => list.clone(),
                        _ => Vec::new(),
                    };
                    let mut added = BTreeMap::from([("id".to_owned(), Tag::String(effect))]);
                    if duration != 160 {
                        added.insert("duration".to_owned(), Tag::Int(duration));
                    }
                    list.push(Tag::Compound(added));
                    set_component(&mut stack, "minecraft:suspicious_stew_effects", Some(Tag::List(list)));
                }
            }
            // `SetInstrumentFunction`: one of the options
            // (`HolderSet.getRandomElement`).
            "set_instrument" => {
                let options = holder_set(&self.registries.datapack, "instrument", function.get("options").ok_or("set_instrument without options")?);
                if !options.is_empty() {
                    let chosen = options[self.random.next_i32_bound(options.len() as i32) as usize].clone();
                    set_component(&mut stack, "minecraft:instrument", Some(Tag::String(chosen)));
                }
            }
            // `SetOminousBottleAmplifierFunction`, 0 to 4. An ominous
            // bottle's own is 0, which is no change to it.
            "set_ominous_bottle_amplifier" => {
                let amplifier = self.int(function.get("amplifier").ok_or("set_ominous_bottle_amplifier without amplifier")?)?.clamp(0, 4);
                let default = amplifier == 0 && stack.id == "minecraft:ominous_bottle";
                set_component(&mut stack, "minecraft:ominous_bottle_amplifier", (!default).then_some(Tag::Int(amplifier)));
            }
            // `ExplorationMapFunction`: no structure is looked for (maps are
            // not simulated), so none is found and the stack stays as it is.
            "exploration_map" => {}
            // `FilteredFunction`: the stack through `on_pass` when it meets
            // the filter, else `on_fail`.
            "filtered" => {
                let passes = self.item_predicate(&stack, function.get("item_filter").ok_or("filtered without item_filter")?)?;
                let branch = if passes { function.get("on_pass") } else { function.get("on_fail") };
                stack = self.apply_modifier(branch, stack)?;
            }
            // `DiscardItem`.
            "discard" => stack = ItemStack::empty(),
            other => return Err(format!("loot function {other} is not supported")),
        }
        Ok(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::AnyRandom;
    use crate::registries::DataPaths;

    /// A random that gives the values it is told to, and notes what was
    /// asked of it: each bounded int's bound, and `bool` for a coin toss.
    struct Scripted {
        ints: Vec<i32>,
        bools: Vec<bool>,
        asked: Vec<String>,
    }

    impl RandomSource for Scripted {
        fn next_i32(&mut self) -> i32 {
            unreachable!("not drawn by the code under test")
        }
        fn next_i32_bound(&mut self, bound: i32) -> i32 {
            self.asked.push(bound.to_string());
            self.ints.remove(0)
        }
        fn next_i64(&mut self) -> i64 {
            unreachable!("not drawn by the code under test")
        }
        fn next_f32(&mut self) -> f32 {
            unreachable!("not drawn by the code under test")
        }
        fn next_f64(&mut self) -> f64 {
            unreachable!("not drawn by the code under test")
        }
        fn next_bool(&mut self) -> bool {
            self.asked.push("bool".to_owned());
            self.bools.remove(0)
        }
        fn next_gaussian(&mut self) -> f64 {
            unreachable!("not drawn by the code under test")
        }
    }

    fn named(stacks: &[ItemStack]) -> Vec<(String, i32)> {
        stacks.iter().map(|s| (s.id.trim_start_matches("minecraft:").to_owned(), s.count)).collect()
    }

    /// Worked through `shuffleAndSplitItems` and `Util.shuffle` by hand.
    #[test]
    fn stacks_split_over_the_free_slots_and_shuffle() {
        let mut stacks = vec![ItemStack::new("minecraft:apple", 1), ItemStack::new("minecraft:bone", 10), ItemStack::new("minecraft:coal", 3), ItemStack::empty()];
        let mut random = Scripted { ints: vec![0, 2, 1, 0, 0, 0, 0, 0], bools: vec![true, false, false], asked: Vec::new() };
        shuffle_and_split(&mut stacks, 5, &mut random);
        // Bone (the first of two to split) gives 3 of its 10, keeps 7 to
        // split again and sets the 3 down; then the 7 gives 1 and both
        // parts stay (the 1 without a toss). Five stacks for five slots:
        // coal is never split. Then the shuffle, every draw 0.
        assert_eq!(random.asked, ["2", "5", "bool", "bool", "2", "3", "bool", "5", "4", "3", "2"]);
        assert_eq!(named(&stacks), [("bone".to_owned(), 3), ("bone".to_owned(), 6), ("bone".to_owned(), 1), ("coal".to_owned(), 3), ("apple".to_owned(), 1)]);
        // One stack of two: no draw picks it or its share of one, and the
        // ones are not tossed for.
        let mut stacks = vec![ItemStack::new("minecraft:diamond", 2)];
        let mut random = Scripted { ints: vec![1], bools: Vec::new(), asked: Vec::new() };
        shuffle_and_split(&mut stacks, 3, &mut random);
        assert_eq!(random.asked, ["2"]);
        assert_eq!(named(&stacks), [("diamond".to_owned(), 1), ("diamond".to_owned(), 1)]);
    }

    /// Every container table of the data pack fills a chest; the same seed
    /// fills it the same way, into distinct empty slots.
    #[test]
    fn every_chest_table_fills_a_chest() {
        let Ok(paths) = DataPaths::discover() else { return };
        let Ok(registries) = Registries::load(&paths) else { return };
        let params = LootParams { origin: Some([0.5, 64.5, 0.5]), this_entity: true, biome: Some("minecraft:plains".to_owned()), ..LootParams::default() };
        let empty: Vec<usize> = (0..27).filter(|slot| slot % 5 != 0).collect();
        let tables = registries.datapack.list("loot_table").unwrap();
        let chests: Vec<String> = tables.iter().map(ToString::to_string).filter(|id| id.starts_with("minecraft:chests/")).collect();
        assert!(chests.len() > 50, "{chests:?}");
        let mut filled = 0;
        for id in &chests {
            let fill = |seed: i64| {
                let mut sequences = RandomSequences::new(3);
                let mut random = AnyRandom::new(true, 5);
                registries.loot.fill(&registries, id, &params, seed, &mut sequences, &mut random, &empty)
            };
            let placed = fill(-4_062_519_233_513_498_765).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(placed, fill(-4_062_519_233_513_498_765).unwrap(), "{id}");
            let mut slots: Vec<usize> = placed.iter().map(|(slot, _)| *slot).collect();
            slots.sort_unstable();
            slots.dedup();
            assert_eq!(slots.len(), placed.len(), "{id}");
            for (slot, stack) in &placed {
                assert!(empty.contains(slot) && !stack.is_empty(), "{id}: {slot} {stack:?}");
                assert!(stack.count <= registries.items.max_stack(&stack.id), "{id}: {stack:?}");
            }
            filled += usize::from(!placed.is_empty());
        }
        assert!(filled > chests.len() / 2, "most tables give something");
        // A desert pyramid's chest: enchanted books and items keep their
        // enchantments as the item component.
        let mut found = false;
        for seed in 1..40 {
            let mut sequences = RandomSequences::new(3);
            let mut random = AnyRandom::new(true, 5);
            let placed = registries.loot.fill(&registries, "minecraft:chests/desert_pyramid", &params, seed, &mut sequences, &mut random, &empty).unwrap();
            if let Some((_, book)) = placed.iter().find(|(_, s)| s.id == "minecraft:enchanted_book") {
                let stored = book.components.as_ref().and_then(|c| c.get("minecraft:stored_enchantments")).and_then(Tag::as_compound);
                assert!(stored.is_some_and(|map| map.len() == 1), "{book:?}");
                found = true;
                break;
            }
        }
        assert!(found, "a desert pyramid gives an enchanted book now and then");
    }

    #[test]
    fn vanilla_block_tables_roll() {
        let Ok(paths) = DataPaths::discover() else { return };
        let Ok(registries) = Registries::load(&paths) else { return };
        let mut sequences = RandomSequences::new(1);
        let mut random = AnyRandom::new(true, 5);
        let params = LootParams { tool: Some(ItemStack::empty()), ..LootParams::default() };
        for name in ["melon", "cobweb", "torch", "oak_leaves", "gravel", "wheat", "diamond_ore", "acacia_slab", "short_grass"] {
            let state = registries.blocks.parse_state(&format!("minecraft:{name}")).unwrap();
            let drops = registries.loot.block_drops(&registries, state, &params, &mut sequences, &mut random);
            assert!(drops.is_ok(), "{name}: {drops:?}");
        }
    }
}
