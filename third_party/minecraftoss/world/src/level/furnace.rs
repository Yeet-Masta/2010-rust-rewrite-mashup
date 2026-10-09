//! Furnaces, smokers and blast furnaces on the server level (26.3
//! `AbstractFurnaceBlockEntity`): the block entity's tick, its block's `lit`
//! state, the recipes it used and the experience they pay, and its
//! `WorldlyContainer` faces and slot rules.
//!
//! What furnaces cook and burn comes from the server's recipe manager and
//! the items' `cooking_fuel` components, through [`Cooking`]. A level
//! without one cooks nothing, though a lit furnace still burns down.

use super::container::{ContainerRef, Stack, Store};
use super::Level;
use minecraftoss_core::nbt::Tag;
use minecraftoss_core::pos::Direction;
use minecraftoss_core::random::RandomSource;
use minecraftoss_core::{BlockPos, BlockStateId};
use std::collections::BTreeMap;

/// `AbstractFurnaceBlockEntity.BURN_TIME_STANDARD`: the cooking time when no
/// recipe takes the ingredient, and `getLitProgress`'s divisor when unlit.
pub const BURN_TIME_STANDARD: i32 = 200;

/// `BURN_COOL_SPEED`: how fast an unlit furnace's progress falls back.
const BURN_COOL_SPEED: i32 = 2;

/// The recipe type a furnace cooks (`RecipeType.SMELTING`, `BLASTING` and
/// `SMOKING`), by its block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CookingType {
    Smelting,
    Blasting,
    Smoking,
}

impl CookingType {
    /// The `block/fast_cooking` predicate (smokers and blast furnaces): fuel
    /// burns half as long, at twice the speed
    /// (`cooking/fast_burn_time_reduction_factor`, `fast_speed_multiplier`).
    pub fn fast(self) -> bool {
        self != Self::Smelting
    }
}

/// A cooking recipe (`AbstractCookingRecipe`) as a furnace reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct CookingRecipe {
    /// Its key (`RecipeHolder.id`), as `RecipesUsed` counts it.
    pub id: String,
    pub result: Stack,
    /// `cookingTime`, in ticks.
    pub cooking_time: i32,
    pub experience: f32,
}

/// The recipe manager and the item data furnaces read.
pub trait Cooking: Send + Sync {
    /// `RecipeManager.CachedCheck.getRecipeFor`: the recipe of the type that
    /// takes the ingredient.
    fn recipe_for(&self, kind: CookingType, ingredient: &Stack) -> Option<CookingRecipe>;
    /// `recipeAccess().byKey`: a cooking recipe by its key.
    fn recipe(&self, id: &str) -> Option<CookingRecipe>;
    /// Whether the item has a `cooking_fuel` component.
    fn is_fuel(&self, id: &str) -> bool;
    /// Its `cooking_fuel` burn time in a plain furnace.
    fn burn_time(&self, id: &str) -> i32;
    /// Its `cooking_fuel` speed multiplier in a plain furnace.
    fn speed_multiplier(&self, id: &str) -> f32;
    /// `Item.getCraftingRemainder` (a lava bucket's bucket).
    fn remainder(&self, id: &str) -> Option<Stack>;
    /// `#furnace_fuel_bottom_takeable`: fuel a hopper below may take.
    fn bottom_takeable(&self, id: &str) -> bool;
}

/// No recipes and no fuel.
struct NoCooking;

impl Cooking for NoCooking {
    fn recipe_for(&self, _kind: CookingType, _ingredient: &Stack) -> Option<CookingRecipe> {
        None
    }
    fn recipe(&self, _id: &str) -> Option<CookingRecipe> {
        None
    }
    fn is_fuel(&self, _id: &str) -> bool {
        false
    }
    fn burn_time(&self, _id: &str) -> i32 {
        0
    }
    fn speed_multiplier(&self, _id: &str) -> f32 {
        1.0
    }
    fn remainder(&self, _id: &str) -> Option<Stack> {
        None
    }
    fn bottom_takeable(&self, _id: &str) -> bool {
        false
    }
}

/// `getSlotsForFace`: the top takes the ingredient, the sides the fuel, and
/// the bottom gives the result, then the fuel.
pub fn slots_for_face(direction: Direction) -> &'static [usize] {
    match direction {
        Direction::Down => &[2, 1],
        Direction::Up => &[0],
        _ => &[1],
    }
}

/// A furnace block entity's state, as it saves it.
#[derive(Clone, Debug, PartialEq)]
pub struct Furnace {
    /// The ingredient, the fuel and the result.
    pub items: [Stack; 3],
    pub lit_time_remaining: i32,
    pub lit_total_time: i32,
    /// `cookingTimer` (saved as `cooking_time_spent`).
    pub cooking_timer: i32,
    pub cooking_total_time: i32,
    pub speed_multiplier: f32,
    /// `RecipesUsed`: the recipes cooked, and how often, since the player
    /// last took their experience.
    pub recipes_used: BTreeMap<String, i32>,
}

impl Default for Furnace {
    fn default() -> Self {
        Self {
            items: [Stack::empty(), Stack::empty(), Stack::empty()],
            lit_time_remaining: 0,
            lit_total_time: 0,
            cooking_timer: 0,
            cooking_total_time: 0,
            speed_multiplier: 1.0,
            recipes_used: BTreeMap::new(),
        }
    }
}

/// What a tick did besides change the furnace's state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ticked {
    /// `setChanged`: comparators read the furnace again.
    pub changed: bool,
    /// The block's `lit` state, when it changed.
    pub lit: Option<bool>,
    /// A fuel's remainder with no room in the fuel slot, dropped at the
    /// block (`Containers.dropItemStack`).
    pub dropped: Option<Stack>,
}

impl Furnace {
    /// `loadAdditional`.
    pub fn from_tag(tag: &Tag) -> Self {
        let int = |key: &str| tag.get(key).and_then(Tag::as_i64).unwrap_or(0) as i32;
        let mut furnace = Self {
            lit_time_remaining: int("lit_time_remaining"),
            lit_total_time: int("lit_total_time"),
            cooking_timer: int("cooking_time_spent"),
            cooking_total_time: int("cooking_total_time"),
            speed_multiplier: tag.get("speed_multiplier").and_then(Tag::as_f64).map_or(1.0, |v| v as f32),
            ..Self::default()
        };
        for (slot, stack) in tag.get("Items").and_then(Tag::as_list).into_iter().flatten().filter_map(Stack::from_tag) {
            if let Some(item) = furnace.items.get_mut(slot) {
                *item = stack;
            }
        }
        for (id, count) in tag.get("RecipesUsed").and_then(Tag::as_compound).into_iter().flatten() {
            if let Some(count) = count.as_i64() {
                furnace.recipes_used.insert(id.clone(), count as i32);
            }
        }
        furnace
    }

    /// `saveAdditional`, into the block entity's tag.
    pub fn save(&self, map: &mut BTreeMap<String, Tag>) {
        map.insert("cooking_time_spent".to_owned(), Tag::Int(self.cooking_timer));
        map.insert("cooking_total_time".to_owned(), Tag::Int(self.cooking_total_time));
        map.insert("lit_time_remaining".to_owned(), Tag::Int(self.lit_time_remaining));
        map.insert("lit_total_time".to_owned(), Tag::Int(self.lit_total_time));
        map.insert("speed_multiplier".to_owned(), Tag::Float(self.speed_multiplier));
        let items = self.items.iter().enumerate().filter(|(_, s)| !s.is_empty()).map(|(i, s)| s.to_tag(i)).collect();
        map.insert("Items".to_owned(), Tag::List(items));
        let used = self.recipes_used.iter().map(|(id, count)| (id.clone(), Tag::Int(*count))).collect();
        map.insert("RecipesUsed".to_owned(), Tag::Compound(used));
    }

    /// `dataAccess`: the menu's data values, in data-slot order.
    pub fn data(&self) -> [i32; 4] {
        [self.lit_time_remaining, self.lit_total_time, self.cooking_timer, self.cooking_total_time]
    }

    pub fn is_lit(&self) -> bool {
        self.lit_time_remaining > 0
    }

    /// `getTotalCookTime(recipe, entity)`: the recipe's time at the last
    /// fuel's speed.
    fn total_cook_time(&self, recipe: &CookingRecipe) -> i32 {
        if self.speed_multiplier > 0.0 {
            (recipe.cooking_time as f32 / self.speed_multiplier).ceil() as i32
        } else {
            recipe.cooking_time
        }
    }

    /// `getTotalCookTime(level, entity)`: for the ingredient there now.
    fn ingredient_cook_time(&self, kind: CookingType, cooking: &dyn Cooking) -> i32 {
        cooking.recipe_for(kind, &self.items[0]).map_or(BURN_TIME_STANDARD, |recipe| self.total_cook_time(&recipe))
    }

    /// `setItem(0, stack)`'s rule: a new ingredient (not a top-up of the
    /// same) starts cooking from nothing. `old` is the stack it replaced.
    pub fn ingredient_set(&mut self, old: &Stack, kind: CookingType, cooking: &dyn Cooking) -> bool {
        let new = &self.items[0];
        let same = !new.is_empty() && old.same_item_same_components(new);
        if same {
            return false;
        }
        self.cooking_total_time = self.ingredient_cook_time(kind, cooking);
        self.cooking_timer = 0;
        true
    }

    /// `canPlaceItem`: nothing goes into the result, and only fuel or a
    /// bucket (one at a time) into the fuel slot.
    pub fn can_place(&self, slot: usize, stack: &Stack, cooking: &dyn Cooking) -> bool {
        match slot {
            2 => false,
            1 => cooking.is_fuel(&stack.id) || stack.id == "minecraft:bucket" && self.items[1].id != "minecraft:bucket",
            _ => true,
        }
    }

    /// `AbstractFurnaceBlockEntity.serverTick`, less the level's part: the
    /// lit time burns down; an unlit furnace with fuel and an ingredient
    /// whose result fits lights from the next fuel; a lit one cooks, and
    /// one with nothing to burn cools. `max_stack` is the item's maximum
    /// stack size.
    pub fn server_tick(&mut self, kind: CookingType, cooking: &dyn Cooking, max_stack: impl Fn(&Stack) -> i32) -> Ticked {
        let mut ticked = Ticked::default();
        let was_lit = self.lit_time_remaining > 0;
        if was_lit {
            self.lit_time_remaining -= 1;
        }
        let mut lit = self.lit_time_remaining > 0;
        let has_ingredient = !self.items[0].is_empty();
        let has_fuel = !self.items[1].is_empty();
        if lit || has_fuel && has_ingredient {
            let recipe = if has_ingredient { cooking.recipe_for(kind, &self.items[0]) } else { None };
            match recipe {
                Some(recipe) if !recipe.result.is_empty() && self.can_burn(&recipe.result, &max_stack) => {
                    if !lit {
                        let fuel = &self.items[1];
                        let is_fuel = cooking.is_fuel(&fuel.id);
                        // `getBurnDuration` and `getSpeedMultiplier`: 0 and
                        // 1 for an item without `cooking_fuel`.
                        let burn = if is_fuel { cooking.burn_time(&fuel.id) / if kind.fast() { 2 } else { 1 } } else { 0 };
                        let speed = if is_fuel { cooking.speed_multiplier(&fuel.id) * if kind.fast() { 2.0 } else { 1.0 } } else { 1.0 };
                        self.lit_time_remaining = burn;
                        self.lit_total_time = burn;
                        self.speed_multiplier = speed;
                        if self.cooking_total_time > 0 && self.cooking_timer < self.cooking_total_time {
                            let ratio = self.cooking_timer as f32 / self.cooking_total_time as f32;
                            self.cooking_total_time = self.total_cook_time(&recipe);
                            self.cooking_timer = (ratio * self.cooking_total_time as f32).ceil() as i32;
                        }
                        if burn > 0 {
                            ticked.dropped = self.consume_fuel(cooking);
                            lit = true;
                            ticked.changed = true;
                        }
                    }
                    if lit {
                        self.cooking_timer += 1;
                        if self.cooking_timer >= self.cooking_total_time {
                            self.cooking_timer = 0;
                            self.cooking_total_time = self.total_cook_time(&recipe);
                            self.burn(&recipe.result);
                            *self.recipes_used.entry(recipe.id).or_insert(0) += 1;
                            ticked.changed = true;
                        }
                    } else {
                        self.cooking_timer = 0;
                    }
                }
                _ => self.cooking_timer = 0,
            }
        } else if self.cooking_timer > 0 {
            self.cooking_timer = (self.cooking_timer - BURN_COOL_SPEED).clamp(0, self.cooking_total_time.max(0));
        }
        if was_lit != lit {
            ticked.changed = true;
            ticked.lit = Some(lit);
        }
        ticked
    }

    /// `canBurn`: the result slot is empty, or holds the same and has room
    /// (`getMaxStackSize`, 99, and the result's own).
    fn can_burn(&self, result: &Stack, max_stack: &impl Fn(&Stack) -> i32) -> bool {
        let current = &self.items[2];
        if current.is_empty() {
            return true;
        }
        if !current.same_item_same_components(result) {
            return false;
        }
        current.count + result.count <= max_stack(result).min(99)
    }

    /// `consumeFuel`: one fuel burns; its remainder takes its place when it
    /// was the last, or is returned to drop.
    fn consume_fuel(&mut self, cooking: &dyn Cooking) -> Option<Stack> {
        let remainder = cooking.remainder(&self.items[1].id);
        self.items[1].count -= 1;
        let emptied = self.items[1].is_empty();
        if emptied {
            self.items[1] = Stack::empty();
        }
        let remainder = remainder?;
        if emptied {
            self.items[1] = remainder;
            None
        } else {
            Some(remainder)
        }
    }

    /// `burn`: the result grows, a wet sponge fills a bucket in the fuel
    /// slot, and one ingredient goes.
    fn burn(&mut self, result: &Stack) {
        if self.items[2].is_empty() {
            self.items[2] = result.clone();
        } else {
            self.items[2].count += result.count;
        }
        if self.items[0].id == "minecraft:wet_sponge" && !self.items[1].is_empty() && self.items[1].id == "minecraft:bucket" {
            self.items[1] = Stack::new("minecraft:water_bucket", 1);
        }
        self.items[0].count -= 1;
        if self.items[0].is_empty() {
            self.items[0] = Stack::empty();
        }
    }
}

/// `createExperience`: `count` times `experience`, its fraction a chance of
/// one more, given a roll of the level random.
pub fn experience(count: i32, value: f32, roll: impl FnOnce() -> f32) -> i32 {
    let total = count as f32 * value;
    let whole = total.floor();
    let fraction = total - whole;
    whole as i32 + i32::from(fraction != 0.0 && roll() < fraction)
}

impl Level<'_> {
    /// The furnace's recipe type, by its block; none for another block.
    pub fn cooking_type(&self, state: BlockStateId) -> Option<CookingType> {
        let blocks = &self.registries().blocks;
        let info = blocks.block(blocks.block_of(state));
        if info.is_a("BlastFurnaceBlock") {
            Some(CookingType::Blasting)
        } else if info.is_a("SmokerBlock") {
            Some(CookingType::Smoking)
        } else if info.is_a("FurnaceBlock") {
            Some(CookingType::Smelting)
        } else {
            None
        }
    }

    fn cooking_source(&self) -> std::sync::Arc<dyn Cooking> {
        self.cooking.clone().unwrap_or_else(|| std::sync::Arc::new(NoCooking))
    }

    /// The furnace block entity at a position.
    pub fn furnace(&self, pos: BlockPos) -> Option<Furnace> {
        self.cooking_type(self.block(pos))?;
        self.block_entity(pos).map(Furnace::from_tag)
    }

    /// Writes the furnace's state into its block entity, when it changed.
    fn put_furnace(&mut self, pos: BlockPos, furnace: &Furnace) {
        if self.furnace(pos).as_ref() == Some(furnace) {
            return;
        }
        if let Some(Tag::Compound(map)) = self.block_entity_mut(pos) {
            furnace.save(map);
        }
    }

    /// The furnace's `ContainerData` for its menu.
    pub fn furnace_data(&self, pos: BlockPos) -> Option<[i32; 4]> {
        self.furnace(pos).map(|furnace| furnace.data())
    }

    /// `AbstractFurnaceBlockEntity.serverTick`.
    pub(super) fn furnace_tick(&mut self, pos: BlockPos) {
        let state = self.block(pos);
        let (Some(kind), Some(mut furnace)) = (self.cooking_type(state), self.furnace(pos)) else { return };
        let cooking = self.cooking_source();
        let ticked = furnace.server_tick(kind, cooking.as_ref(), |stack| self.item_max_stack(stack));
        self.put_furnace(pos, &furnace);
        if let Some(stack) = ticked.dropped {
            self.drop_item_stack([f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)], stack);
        }
        if let Some(lit) = ticked.lit {
            // `setBlockAndUpdate` with the `LIT` state.
            let lit_state = self.with(state, "lit", if lit { "true" } else { "false" });
            self.set_block_and_update(pos, lit_state);
        }
        if ticked.changed {
            self.block_entity_changed(pos);
        }
    }

    /// `AbstractFurnaceBlockEntity.setItem` for the ingredient: `old` was
    /// replaced by what the slot now holds.
    pub(super) fn furnace_ingredient_set(&mut self, pos: BlockPos, old: &Stack) {
        let (Some(kind), Some(mut furnace)) = (self.cooking_type(self.block(pos)), self.furnace(pos)) else { return };
        let cooking = self.cooking_source();
        if furnace.ingredient_set(old, kind, cooking.as_ref()) {
            self.put_furnace(pos, &furnace);
        }
    }

    /// `canPlaceItem` for a furnace's slot.
    pub(super) fn furnace_can_place(&self, pos: BlockPos, slot: usize, stack: &Stack) -> bool {
        let cooking = self.cooking_source();
        self.furnace(pos).is_some_and(|furnace| furnace.can_place(slot, stack, cooking.as_ref()))
    }

    /// `canTakeItemThroughFace`: from below, only the fuels a hopper may
    /// take (empty and water buckets).
    pub(super) fn furnace_can_take(&self, slot: usize, stack: &Stack, direction: Direction) -> bool {
        direction != Direction::Down || slot != 1 || self.cooking_source().bottom_takeable(&stack.id)
    }

    /// `getRecipesToAwardAndPopExperience`: each recipe the furnace used pays
    /// its experience as orbs at `at`. Returns the recipes.
    fn pop_used_recipe_experience(&mut self, pos: BlockPos, at: [f64; 3]) -> Vec<String> {
        let Some(furnace) = self.furnace(pos) else { return Vec::new() };
        let cooking = self.cooking_source();
        let mut awarded = Vec::new();
        for (id, count) in &furnace.recipes_used {
            let Some(recipe) = cooking.recipe(id) else { continue };
            awarded.push(id.clone());
            let amount = experience(*count, recipe.experience, || self.random.next_f32());
            self.award_experience(at, amount);
        }
        awarded
    }

    /// `awardUsedRecipesAndPopExperience`: the player took from the result.
    /// The used recipes pay their experience at the player's position and
    /// are forgotten; returns them for the player to unlock (`awardRecipes`).
    pub fn award_used_recipes(&mut self, pos: BlockPos, player: [f64; 3]) -> Vec<String> {
        let awarded = self.pop_used_recipe_experience(pos, player);
        if let Some(mut furnace) = self.furnace(pos) {
            furnace.recipes_used.clear();
            self.put_furnace(pos, &furnace);
        }
        awarded
    }

    /// `AbstractFurnaceBlockEntity.preRemoveSideEffects`' own part: the used
    /// recipes' experience pops at the block's centre.
    pub(super) fn furnace_removed(&mut self, pos: BlockPos) {
        let centre = [f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5];
        self.pop_used_recipe_experience(pos, centre);
    }

    pub(super) fn is_furnace(&self, c: ContainerRef) -> Option<BlockPos> {
        match c {
            ContainerRef::Single(pos, Store::Furnace) => Some(pos),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Iron ore smelts (and blasts) to an iron ingot in 200 ticks for 0.7
    /// experience; coal burns 1600 ticks, a lava bucket 20000 leaving its
    /// bucket; a wet sponge dries.
    struct Book;

    impl Cooking for Book {
        fn recipe_for(&self, kind: CookingType, ingredient: &Stack) -> Option<CookingRecipe> {
            match (kind, ingredient.id.as_str()) {
                (CookingType::Smelting | CookingType::Blasting, "minecraft:iron_ore") => self.recipe("minecraft:iron_ingot_from_smelting_iron_ore"),
                (CookingType::Smelting, "minecraft:wet_sponge") => Some(CookingRecipe { id: "minecraft:sponge".to_owned(), result: Stack::new("minecraft:sponge", 1), cooking_time: 200, experience: 0.15 }),
                _ => None,
            }
        }
        fn recipe(&self, id: &str) -> Option<CookingRecipe> {
            (id == "minecraft:iron_ingot_from_smelting_iron_ore").then(|| CookingRecipe { id: id.to_owned(), result: Stack::new("minecraft:iron_ingot", 1), cooking_time: 200, experience: 0.7 })
        }
        fn is_fuel(&self, id: &str) -> bool {
            self.burn_time(id) > 0
        }
        fn burn_time(&self, id: &str) -> i32 {
            match id {
                "minecraft:coal" => 1600,
                "minecraft:lava_bucket" => 20000,
                "minecraft:dried_kelp_block" => 4001,
                _ => 0,
            }
        }
        fn speed_multiplier(&self, _id: &str) -> f32 {
            1.0
        }
        fn remainder(&self, id: &str) -> Option<Stack> {
            (id == "minecraft:lava_bucket").then(|| Stack::new("minecraft:bucket", 1))
        }
        fn bottom_takeable(&self, id: &str) -> bool {
            id == "minecraft:bucket" || id == "minecraft:water_bucket"
        }
    }

    fn loaded(ingredient: Stack, fuel: Stack) -> Furnace {
        let mut furnace = Furnace { items: [ingredient, fuel, Stack::empty()], ..Furnace::default() };
        // As the menu's `setItem` leaves a new ingredient.
        furnace.ingredient_set(&Stack::empty(), CookingType::Smelting, &Book);
        furnace
    }

    fn tick(furnace: &mut Furnace, kind: CookingType) -> Ticked {
        furnace.server_tick(kind, &Book, |_| 64)
    }

    #[test]
    fn a_furnace_lights_cooks_and_records_the_recipe() {
        let mut furnace = loaded(Stack::new("minecraft:iron_ore", 2), Stack::new("minecraft:coal", 1));
        assert_eq!(furnace.cooking_total_time, 200);
        let first = tick(&mut furnace, CookingType::Smelting);
        assert_eq!(first, Ticked { changed: true, lit: Some(true), dropped: None });
        assert_eq!(furnace.data(), [1600, 1600, 1, 200], "it lights and cooks in the same tick");
        assert!(furnace.items[1].is_empty(), "the coal burns");
        for _ in 1..199 {
            assert_eq!(tick(&mut furnace, CookingType::Smelting), Ticked::default());
        }
        assert!(furnace.items[2].is_empty());
        assert_eq!(tick(&mut furnace, CookingType::Smelting), Ticked { changed: true, lit: None, dropped: None });
        assert_eq!(furnace.items[2], Stack::new("minecraft:iron_ingot", 1));
        assert_eq!(furnace.items[0].count, 1);
        assert_eq!(furnace.data(), [1401, 1600, 0, 200]);
        assert_eq!(furnace.recipes_used.get("minecraft:iron_ingot_from_smelting_iron_ore"), Some(&1));
    }

    #[test]
    fn fast_furnaces_burn_fuel_half_as_long_at_twice_the_speed() {
        let mut furnace = loaded(Stack::new("minecraft:iron_ore", 1), Stack::new("minecraft:dried_kelp_block", 1));
        tick(&mut furnace, CookingType::Blasting);
        assert_eq!(furnace.lit_total_time, 2000, "4001 / 2, rounded down");
        assert_eq!(furnace.speed_multiplier, 2.0);
        // The total kept its ratio when the fuel lit: 1 of 100.
        assert_eq!((furnace.cooking_timer, furnace.cooking_total_time), (1, 100));
        for _ in 1..100 {
            tick(&mut furnace, CookingType::Blasting);
        }
        assert_eq!(furnace.items[2], Stack::new("minecraft:iron_ingot", 1), "half the cooking time");
        // A smoker has no recipe for iron ore: it neither lights nor cooks.
        let mut smoker = loaded(Stack::new("minecraft:iron_ore", 1), Stack::new("minecraft:coal", 1));
        assert_eq!(tick(&mut smoker, CookingType::Smoking), Ticked::default());
        assert_eq!(smoker.items[1].count, 1);
    }

    #[test]
    fn an_unlit_furnace_cools_and_goes_out() {
        let mut furnace = loaded(Stack::new("minecraft:iron_ore", 1), Stack::new("minecraft:coal", 1));
        furnace.lit_time_remaining = 1;
        furnace.lit_total_time = 1600;
        furnace.items[1] = Stack::empty();
        furnace.cooking_timer = 5;
        assert_eq!(tick(&mut furnace, CookingType::Smelting), Ticked { changed: true, lit: Some(false), dropped: None }, "out of fuel, it goes out");
        assert_eq!(furnace.cooking_timer, 3, "and cools by 2 a tick");
        tick(&mut furnace, CookingType::Smelting);
        tick(&mut furnace, CookingType::Smelting);
        assert_eq!(furnace.cooking_timer, 0);
        // A lit furnace whose result has no room stops its progress.
        let mut full = loaded(Stack::new("minecraft:iron_ore", 1), Stack::new("minecraft:coal", 1));
        full.items[2] = Stack::new("minecraft:iron_ingot", 64);
        full.lit_time_remaining = 10;
        full.cooking_timer = 50;
        tick(&mut full, CookingType::Smelting);
        assert_eq!((full.lit_time_remaining, full.cooking_timer), (9, 0));
    }

    #[test]
    fn fuel_leaves_its_remainder_and_a_wet_sponge_fills_a_bucket() {
        let mut furnace = loaded(Stack::new("minecraft:iron_ore", 1), Stack::new("minecraft:lava_bucket", 1));
        tick(&mut furnace, CookingType::Smelting);
        assert_eq!(furnace.items[1], Stack::new("minecraft:bucket", 1), "the last lava bucket leaves its bucket");
        let mut two = loaded(Stack::new("minecraft:iron_ore", 1), Stack::new("minecraft:lava_bucket", 2));
        assert_eq!(tick(&mut two, CookingType::Smelting).dropped, Some(Stack::new("minecraft:bucket", 1)), "with more left, it drops");
        let mut sponge = loaded(Stack::new("minecraft:wet_sponge", 1), Stack::new("minecraft:bucket", 1));
        sponge.lit_time_remaining = 300;
        sponge.cooking_timer = 199;
        tick(&mut sponge, CookingType::Smelting);
        assert_eq!((sponge.items[1].id.as_str(), sponge.items[2].id.as_str()), ("minecraft:water_bucket", "minecraft:sponge"));
    }

    #[test]
    fn a_new_ingredient_restarts_cooking_and_fuel_slots_take_fuel() {
        let mut furnace = loaded(Stack::new("minecraft:iron_ore", 1), Stack::empty());
        furnace.cooking_timer = 120;
        let old = furnace.items[0].clone();
        furnace.items[0].count = 5;
        assert!(!furnace.ingredient_set(&old, CookingType::Smelting, &Book), "a top-up keeps cooking");
        furnace.items[0] = Stack::new("minecraft:stone", 1);
        assert!(furnace.ingredient_set(&old, CookingType::Smelting, &Book));
        assert_eq!((furnace.cooking_timer, furnace.cooking_total_time), (0, BURN_TIME_STANDARD));
        assert!(furnace.can_place(1, &Stack::new("minecraft:coal", 1), &Book));
        assert!(furnace.can_place(1, &Stack::new("minecraft:bucket", 1), &Book));
        assert!(!furnace.can_place(1, &Stack::new("minecraft:stone", 1), &Book));
        assert!(!furnace.can_place(2, &Stack::new("minecraft:coal", 1), &Book));
        furnace.items[1] = Stack::new("minecraft:bucket", 1);
        assert!(!furnace.can_place(1, &Stack::new("minecraft:bucket", 1), &Book), "one bucket at a time");
        assert_eq!(slots_for_face(Direction::Down), &[2, 1]);
        assert_eq!(slots_for_face(Direction::Up), &[0]);
        assert_eq!(slots_for_face(Direction::East), &[1]);
    }

    #[test]
    fn experience_rounds_its_fraction_by_chance() {
        assert_eq!(experience(3, 0.7, || 0.5), 2, "2.1: 0.5 misses the 0.1");
        assert_eq!(experience(3, 0.7, || 0.05), 3);
        assert_eq!(experience(10, 0.1, || panic!("whole")), 1);
    }

    #[test]
    fn the_state_saves_as_the_block_entity_does() {
        let mut furnace = loaded(Stack::new("minecraft:iron_ore", 3), Stack::new("minecraft:coal", 1));
        furnace.recipes_used.insert("minecraft:iron_ingot_from_smelting_iron_ore".to_owned(), 2);
        furnace.speed_multiplier = 2.0;
        let mut map = BTreeMap::new();
        furnace.save(&mut map);
        assert_eq!(map.get("cooking_total_time"), Some(&Tag::Int(200)));
        assert_eq!(Furnace::from_tag(&Tag::Compound(map)), furnace);
    }
}
