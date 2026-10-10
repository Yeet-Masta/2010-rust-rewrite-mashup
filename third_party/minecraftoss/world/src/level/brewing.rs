//! Brewing stands on the server level (26.3 `BrewingStandBlockEntity`): the
//! fuel taken as it runs out, the brew started, abandoned or finished, the
//! bottles each brewed by their recipe, the reagent used up with its
//! remainder, the `has_bottle_0..2` states, the brewing sound, and the
//! `WorldlyContainer` faces and slot rules.
//!
//! What brews comes from the server's recipe manager and the items'
//! `brewing_fuel` components, through [`Brewing`]. A level without one
//! brews nothing.

use super::container::Stack;
use super::{update, Level};
use minecraftoss_core::nbt::Tag;
use minecraftoss_core::pos::Direction;
use minecraftoss_core::BlockPos;
use std::collections::BTreeMap;

/// `BrewingStandBlockEntity`'s brewing time at speed 1, in ticks.
const BREWING_TIME: f32 = 400.0;
/// `LevelEvent.SOUND_BREWING_STAND_BREW`.
pub const BREWING_STAND_BREW: i32 = 1035;

const INGREDIENT: usize = 3;
const FUEL: usize = 4;

/// The recipe manager and the item data brewing stands read.
pub trait Brewing: Send + Sync {
    /// `RecipeManager.CachedCheck.getRecipeFor(BrewingInput)` with
    /// `assemble`: what the bottle brews into with the reagent.
    fn brew(&self, bottle: &Stack, reagent: &Stack) -> Option<Stack>;
    /// The `brewing_reagent` recipe property set: every brewing recipe's
    /// reagent.
    fn is_reagent(&self, stack: &Stack) -> bool;
    /// `PotionIngredient.isPotionInput`: a brewing recipe's bottle, or in
    /// `#brewing_potion_inputs`.
    fn is_potion_input(&self, stack: &Stack) -> bool;
    /// The stack's `brewing_fuel`: its uses and speed multiplier.
    fn fuel(&self, stack: &Stack) -> Option<(i32, f32)>;
    /// `Item.getCraftingRemainder`.
    fn remainder(&self, id: &str) -> Option<Stack>;
}

/// `getSlotsForFace`: the top takes the reagent, the bottom gives the
/// bottles and the reagent, and the sides take the bottles and the fuel.
pub fn slots_for_face(direction: Direction) -> &'static [usize] {
    match direction {
        Direction::Up => &[3],
        Direction::Down => &[0, 1, 2, 3],
        _ => &[0, 1, 2, 4],
    }
}

/// `canTakeItemThroughFace`: the reagent's slot gives only a glass bottle
/// (dragon's breath's remainder).
pub fn can_take(slot: usize, stack: &Stack) -> bool {
    slot != INGREDIENT || stack.id == "minecraft:glass_bottle"
}

/// A brewing stand block entity's state, as it saves it.
#[derive(Clone, Debug, PartialEq)]
pub struct BrewingStand {
    /// The three bottles, the reagent and the fuel.
    pub items: [Stack; 5],
    pub brew_time: i32,
    pub total_brew_time: i32,
    /// The fuel's uses left, and what it had.
    pub fuel: i32,
    pub total_fuel: i32,
    pub speed_multiplier: f32,
}

/// A new block entity's: nothing brewed, no fuel, and no totals yet.
impl Default for BrewingStand {
    fn default() -> Self {
        Self { items: std::array::from_fn(|_| Stack::empty()), brew_time: 0, total_brew_time: 0, fuel: 0, total_fuel: 0, speed_multiplier: 1.0 }
    }
}

/// What the block entity keeps without saving: the reagent it brews with
/// (`ingredient`) and the bottles its block last showed (`lastPotionCount`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Unsaved {
    pub ingredient: Option<String>,
    pub bottles: Option<[bool; 3]>,
}

impl Unsaved {
    /// As `loadAdditional` leaves it: a stand that was brewing brews with
    /// the reagent it holds.
    pub fn loaded(stand: &BrewingStand) -> Self {
        let ingredient = (stand.brew_time > 0).then(|| stand.items[INGREDIENT].id.clone());
        Self { ingredient, bottles: None }
    }
}

/// What a tick did besides change the stand's state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ticked {
    /// `setChanged`: comparators read the stand again.
    pub changed: bool,
    /// A brew finished: level event 1035.
    pub brewed: bool,
    /// Remainders with no room, dropped at the block
    /// (`Containers.dropItemStack`).
    pub dropped: Vec<Stack>,
    /// The `has_bottle_0..2` states, when they changed.
    pub bottles: Option<[bool; 3]>,
}

impl BrewingStand {
    /// `loadAdditional`.
    pub fn from_tag(tag: &Tag) -> Self {
        let int = |key: &str, default: i32| tag.get(key).and_then(Tag::as_i64).map_or(default, |v| v as i32);
        let mut stand = Self {
            brew_time: int("BrewTime", 0),
            total_brew_time: int("total_brew_time", 400),
            fuel: int("Fuel", 0),
            total_fuel: int("total_fuel", 20),
            speed_multiplier: tag.get("speed_multiplier").and_then(Tag::as_f64).map_or(1.0, |v| v as f32),
            ..Self::default()
        };
        for (slot, stack) in tag.get("Items").and_then(Tag::as_list).into_iter().flatten().filter_map(Stack::from_tag) {
            if let Some(item) = stand.items.get_mut(slot) {
                *item = stack;
            }
        }
        stand
    }

    /// `saveAdditional`, into the block entity's tag.
    pub fn save(&self, map: &mut BTreeMap<String, Tag>) {
        map.insert("BrewTime".to_owned(), Tag::Int(self.brew_time));
        map.insert("total_brew_time".to_owned(), Tag::Int(self.total_brew_time));
        let items = self.items.iter().enumerate().filter(|(_, s)| !s.is_empty()).map(|(i, s)| s.to_tag(i)).collect();
        map.insert("Items".to_owned(), Tag::List(items));
        map.insert("Fuel".to_owned(), Tag::Int(self.fuel));
        map.insert("total_fuel".to_owned(), Tag::Int(self.total_fuel));
        map.insert("speed_multiplier".to_owned(), Tag::Float(self.speed_multiplier));
    }

    /// `dataAccess`: the menu's data values, in data-slot order.
    pub fn data(&self) -> [i32; 4] {
        [self.brew_time, self.fuel, self.total_brew_time, self.total_fuel]
    }

    /// `canPlaceItem`: fuel in the fuel slot, a reagent in the reagent's,
    /// and a potion input in an empty bottle slot.
    pub fn can_place(&self, slot: usize, stack: &Stack, brewing: &dyn Brewing) -> bool {
        match slot {
            FUEL => brewing.fuel(stack).is_some(),
            INGREDIENT => brewing.is_reagent(stack),
            _ => brewing.is_potion_input(stack) && self.items.get(slot).is_some_and(Stack::is_empty),
        }
    }

    /// `isBrewable`: a reagent, and a bottle that brews with it.
    fn is_brewable(&self, brewing: &dyn Brewing) -> bool {
        let reagent = &self.items[INGREDIENT];
        if reagent.is_empty() || !brewing.is_reagent(reagent) {
            return false;
        }
        self.items[..3].iter().any(|bottle| !bottle.is_empty() && brewing.brew(bottle, reagent).is_some())
    }

    /// `doBrew`: each bottle brews by its recipe (or stays), and one
    /// reagent is used, its remainder in its place when it was the last.
    fn brew(&mut self, brewing: &dyn Brewing, ticked: &mut Ticked) {
        let reagent = self.items[INGREDIENT].clone();
        for bottle in self.items[..3].iter_mut() {
            if let Some(brewed) = brewing.brew(bottle, &reagent) {
                *bottle = brewed;
            }
        }
        let remainder = brewing.remainder(&reagent.id);
        let mut left = reagent;
        left.count -= 1;
        if left.is_empty() {
            left = Stack::empty();
        }
        if let Some(remainder) = remainder {
            if left.is_empty() {
                left = remainder;
            } else {
                ticked.dropped.push(remainder);
            }
        }
        self.items[INGREDIENT] = left;
        ticked.brewed = true;
    }

    /// `BrewingStandBlockEntity.serverTick`, less the level's part: an
    /// empty fuel takes the next; a brew counts down and finishes, or is
    /// abandoned when the stand can no longer brew or the reagent changed;
    /// a stand that can brew starts on one fuel use, for `400 / speed`
    /// ticks; the block shows the bottles.
    pub fn server_tick(&mut self, unsaved: &mut Unsaved, brewing: &dyn Brewing) -> Ticked {
        let mut ticked = Ticked::default();
        let fuel = &self.items[FUEL];
        if let Some((uses, speed)) = brewing.fuel(fuel).filter(|_| self.fuel <= 0) {
            self.fuel = uses;
            self.total_fuel = uses;
            self.speed_multiplier = speed;
            let remainder = brewing.remainder(&fuel.id);
            let mut left = fuel.clone();
            left.count -= 1;
            if left.is_empty() {
                left = Stack::empty();
            }
            if let Some(remainder) = remainder {
                if left.is_empty() {
                    left = remainder;
                } else {
                    ticked.dropped.push(remainder);
                }
            }
            self.items[FUEL] = left;
            ticked.changed = true;
        }
        let brewable = self.is_brewable(brewing);
        let reagent = self.items[INGREDIENT].clone();
        if self.brew_time > 0 {
            self.brew_time -= 1;
            let same = !reagent.is_empty() && unsaved.ingredient.as_deref() == Some(reagent.id.as_str());
            if self.brew_time == 0 && brewable {
                self.brew(brewing, &mut ticked);
            } else if !brewable || !same {
                self.brew_time = 0;
            }
            ticked.changed = true;
        } else if brewable && self.fuel > 0 {
            let speed = if self.speed_multiplier > 0.0 { self.speed_multiplier } else { 1.0 };
            self.fuel -= 1;
            self.brew_time = (BREWING_TIME / speed).ceil() as i32;
            self.total_brew_time = self.brew_time;
            unsaved.ingredient = Some(reagent.id);
            ticked.changed = true;
        }
        let bottles = [0, 1, 2].map(|slot| !self.items[slot].is_empty());
        if unsaved.bottles != Some(bottles) {
            unsaved.bottles = Some(bottles);
            ticked.bottles = Some(bottles);
        }
        ticked
    }
}

impl Level<'_> {
    /// The brewing stand block entity at a position.
    pub fn brewing_stand(&self, pos: BlockPos) -> Option<BrewingStand> {
        if !self.is_a(self.block(pos), "BrewingStandBlock") {
            return None;
        }
        self.block_entity(pos).map(BrewingStand::from_tag)
    }

    /// Writes the stand's state into its block entity, when it changed.
    fn put_brewing_stand(&mut self, pos: BlockPos, stand: &BrewingStand) {
        if self.brewing_stand(pos).as_ref() == Some(stand) {
            return;
        }
        if let Some(Tag::Compound(map)) = self.block_entity_mut(pos) {
            stand.save(map);
        }
    }

    /// The stand's `ContainerData` for its menu.
    pub fn brewing_stand_data(&self, pos: BlockPos) -> Option<[i32; 4]> {
        self.brewing_stand(pos).map(|stand| stand.data())
    }

    /// `BrewingStandBlockEntity.serverTick`.
    pub(super) fn brewing_stand_tick(&mut self, pos: BlockPos) {
        let (Some(mut stand), Some(brewing)) = (self.brewing_stand(pos), self.brewing.clone()) else { return };
        let mut unsaved = self.brewing_unsaved.remove(&pos).unwrap_or_else(|| Unsaved::loaded(&stand));
        let ticked = stand.server_tick(&mut unsaved, brewing.as_ref());
        self.brewing_unsaved.insert(pos, unsaved);
        self.put_brewing_stand(pos, &stand);
        for stack in ticked.dropped {
            self.drop_item_stack([f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)], stack);
        }
        if ticked.brewed {
            self.level_event(BREWING_STAND_BREW, pos, 0);
        }
        if ticked.changed {
            self.block_entity_changed(pos);
        }
        if let Some(bottles) = ticked.bottles {
            // `setBlock(pos, state, 2)` with each `HAS_BOTTLE`.
            let mut state = self.block(pos);
            for (slot, has) in bottles.into_iter().enumerate() {
                state = self.with(state, &format!("has_bottle_{slot}"), if has { "true" } else { "false" });
            }
            self.set_block(pos, state, update::CLIENTS, update::LIMIT);
        }
    }

    /// `canPlaceItem` (and `canPlaceItemThroughFace`) for a stand's slot.
    pub(super) fn brewing_stand_can_place(&self, pos: BlockPos, slot: usize, stack: &Stack) -> bool {
        let Some(brewing) = self.brewing.as_ref() else { return false };
        self.brewing_stand(pos).is_some_and(|stand| stand.can_place(slot, stack, brewing.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Water and nether wart make awkward, and awkward and sugar
    /// swiftness; blaze powder burns 20 times; dragon's breath leaves its
    /// bottle.
    struct Book;

    fn potion(stack: &Stack) -> Option<&str> {
        stack.components.as_ref()?.get("minecraft:potion_contents")?.get("potion")?.as_str()
    }

    fn bottle(name: &str) -> Stack {
        let mut stack = Stack::new("minecraft:potion", 1);
        let contents = Tag::Compound([("potion".to_owned(), Tag::String(format!("minecraft:{name}")))].into());
        stack.components = Some(Tag::Compound([("minecraft:potion_contents".to_owned(), contents)].into()));
        stack
    }

    impl Brewing for Book {
        fn brew(&self, bottle_stack: &Stack, reagent: &Stack) -> Option<Stack> {
            match (potion(bottle_stack)?, reagent.id.as_str()) {
                ("minecraft:water", "minecraft:nether_wart") => Some(bottle("awkward")),
                ("minecraft:awkward", "minecraft:sugar") => Some(bottle("swiftness")),
                ("minecraft:water", "minecraft:dragon_breath") => Some(bottle("water")),
                _ => None,
            }
        }
        fn is_reagent(&self, stack: &Stack) -> bool {
            matches!(stack.id.as_str(), "minecraft:nether_wart" | "minecraft:sugar" | "minecraft:dragon_breath" | "minecraft:blaze_powder")
        }
        fn is_potion_input(&self, stack: &Stack) -> bool {
            matches!(stack.id.as_str(), "minecraft:potion" | "minecraft:glass_bottle")
        }
        fn fuel(&self, stack: &Stack) -> Option<(i32, f32)> {
            (stack.id == "minecraft:blaze_powder").then_some((20, 1.0))
        }
        fn remainder(&self, id: &str) -> Option<Stack> {
            (id == "minecraft:dragon_breath").then(|| Stack::new("minecraft:glass_bottle", 1))
        }
    }

    fn stand(reagent: Stack, fuel: Stack) -> BrewingStand {
        BrewingStand { items: [bottle("water"), Stack::empty(), bottle("water"), reagent, fuel], ..BrewingStand::default() }
    }

    #[test]
    fn a_stand_fuels_brews_for_400_ticks_and_uses_its_reagent() {
        let mut stand = stand(Stack::new("minecraft:nether_wart", 2), Stack::new("minecraft:blaze_powder", 1));
        let mut unsaved = Unsaved::default();
        let first = stand.server_tick(&mut unsaved, &Book);
        assert_eq!(first, Ticked { changed: true, bottles: Some([true, false, true]), ..Ticked::default() });
        assert_eq!(stand.data(), [400, 19, 400, 20], "the powder's 20 uses, one for this brew");
        assert!(stand.items[4].is_empty());
        for _ in 1..400 {
            assert!(!stand.server_tick(&mut unsaved, &Book).brewed);
        }
        let done = stand.server_tick(&mut unsaved, &Book);
        assert!(done.brewed && done.dropped.is_empty());
        assert_eq!((potion(&stand.items[0]), potion(&stand.items[2])), (Some("minecraft:awkward"), Some("minecraft:awkward")));
        assert!(stand.items[1].is_empty(), "an empty slot brews nothing");
        assert_eq!(stand.items[3].count, 1);
        // Awkward potions do not brew with nether wart: it waits.
        assert_eq!(stand.server_tick(&mut unsaved, &Book), Ticked::default());
        assert_eq!(stand.data(), [0, 19, 400, 20]);
    }

    #[test]
    fn a_brew_stops_when_its_reagent_changes() {
        let mut stand = stand(Stack::new("minecraft:nether_wart", 1), Stack::new("minecraft:blaze_powder", 1));
        let mut unsaved = Unsaved::default();
        stand.server_tick(&mut unsaved, &Book);
        stand.items[3] = Stack::new("minecraft:dragon_breath", 1);
        stand.server_tick(&mut unsaved, &Book);
        assert_eq!(stand.brew_time, 0, "dragon's breath brews too, but it is not the reagent begun with");
        // A stand loaded mid-brew brews with the reagent it holds.
        stand.brew_time = 1;
        let mut loaded = Unsaved::loaded(&stand);
        let ticked = stand.server_tick(&mut loaded, &Book);
        assert!(ticked.brewed);
        assert_eq!(stand.items[3], Stack::new("minecraft:glass_bottle", 1), "the last breath leaves its bottle");
        assert_eq!(ticked.bottles, Some([true, false, true]), "a loaded stand sets its block once");
    }

    #[test]
    fn hoppers_reach_the_slots_their_faces_name() {
        let mut stand = stand(Stack::empty(), Stack::empty());
        assert!(stand.can_place(4, &Stack::new("minecraft:blaze_powder", 1), &Book));
        assert!(!stand.can_place(4, &Stack::new("minecraft:sugar", 1), &Book));
        assert!(stand.can_place(3, &Stack::new("minecraft:sugar", 1), &Book));
        assert!(stand.can_place(1, &bottle("water"), &Book));
        assert!(!stand.can_place(0, &bottle("water"), &Book), "it holds one");
        stand.items[1] = Stack::new("minecraft:glass_bottle", 1);
        assert!(!stand.can_place(1, &Stack::new("minecraft:glass_bottle", 1), &Book));
        assert_eq!((slots_for_face(Direction::Up), slots_for_face(Direction::Down), slots_for_face(Direction::West)), (&[3][..], &[0, 1, 2, 3][..], &[0, 1, 2, 4][..]));
        assert!(can_take(0, &bottle("awkward")) && can_take(3, &Stack::new("minecraft:glass_bottle", 1)));
        assert!(!can_take(3, &Stack::new("minecraft:sugar", 1)));
    }

    #[test]
    fn the_state_saves_as_the_block_entity_does() {
        let mut stand = stand(Stack::new("minecraft:sugar", 3), Stack::new("minecraft:blaze_powder", 5));
        stand.brew_time = 120;
        stand.fuel = 7;
        let mut map = BTreeMap::new();
        stand.save(&mut map);
        assert_eq!(map.get("BrewTime"), Some(&Tag::Int(120)));
        assert_eq!(BrewingStand::from_tag(&Tag::Compound(map)), stand);
        assert_eq!(BrewingStand::from_tag(&Tag::Compound(BTreeMap::new())).data(), [0, 0, 400, 20]);
    }
}
