//! `EnchantmentMenu` (26.3): the item and lapis slots, the three offers
//! the bookshelves around the table and the player's enchantment seed make
//! (`slotsChanged`), and the buttons that enchant (`clickMenuButton`).
//!
//! What the table reaches through its `ContainerLevelAccess` comes as a
//! [`TableAccess`]: the enchantment registry ([`EnchantingTable`]) and the
//! bookshelves the server counted around the block ([`bookshelf_count`]).
//! A client's copy has none (`ContainerLevelAccess.NULL`): it checks a
//! button without pressing it, and leaves the offers to the server.
//!
//! Not simulated: the `enchanted_item` advancement trigger.
use super::{
    Menu, MenuContext, OwnSlots, SlotDef, clear_own_slots, item, move_item_stack_to, put_back,
    return_carried, set, standard_inventory_slots,
};
use crate::inventory::ItemStack;
use crate::rng::LegacyRandom;
use serde_json::Value;
use std::sync::Arc;

const ITEM: usize = 0;
const LAPIS: usize = 1;
/// The main inventory is 2-28, the hotbar 29-37.
const INVENTORY: usize = 2;
const END: usize = 38;

/// The data slots: the three costs, the seed, the three clues' enchantments
/// and their levels.
pub const SEED: usize = 3;
const CLUES: usize = 4;
const LEVEL_CLUES: usize = 7;

/// `EMPTY_SLOT_LAPIS_LAZULI`.
pub const LAPIS_ICON: &str = "container/slot/lapis_lazuli";
const LAPIS_LAZULI: &str = "minecraft:lapis_lazuli";

/// The enchantment registry and the items' enchantability, as the table
/// reads them (`level.registryAccess()`, `DataComponents.ENCHANTABLE`).
pub trait EnchantingTable: Send + Sync {
    /// The item's default `enchantable` (`Enchantable.value`); none for an
    /// item without one.
    fn enchantable(&self, item: &str) -> Option<i32>;

    /// `EnchantmentHelper.selectEnchantment(random, stack, cost,
    /// #in_enchanting_table)` for an item whose `enchantable` is
    /// `enchantable`: the chosen (registry id, level), in order.
    fn select(
        &self,
        random: &mut LegacyRandom,
        item: &str,
        enchantable: i32,
        cost: i32,
    ) -> Vec<(i32, i32)>;

    /// The enchantment with a registry id (`asHolderIdMap().byId`).
    fn enchantment(&self, id: i32) -> Option<&str>;
}

/// What `ContainerLevelAccess.execute` reaches on a server.
#[derive(Clone)]
pub struct TableAccess {
    pub table: Arc<dyn EnchantingTable>,
    /// The bookshelves around the table as last counted
    /// ([`bookshelf_count`]).
    pub bookshelves: i32,
}

/// `EnchantingTableBlock.BOOKSHELF_OFFSETS`: the ring two blocks out, on
/// the table's level and the one above, in `BlockPos.betweenClosed`'s order
/// (x fastest, then y, then z).
pub fn bookshelf_offsets() -> impl Iterator<Item = [i32; 3]> {
    (-2..=2)
        .flat_map(|z| (0..=1).flat_map(move |y| (-2..=2).map(move |x| [x, y, z])))
        .filter(|&[x, _, z]: &[i32; 3]| x.abs() == 2 || z.abs() == 2)
}

/// `EnchantingTableBlock.isValidBookShelf`: a power provider (a bookshelf)
/// at the offset, and a power transmitter (a replaceable block, air among
/// them) halfway to it, the halves cut toward zero.
pub fn is_valid_bookshelf(
    offset: [i32; 3],
    provider: impl Fn([i32; 3]) -> bool,
    transmitter: impl Fn([i32; 3]) -> bool,
) -> bool {
    let [x, y, z] = offset;
    provider(offset) && transmitter([x / 2, y, z / 2])
}

/// The bookshelves `slotsChanged` counts around a table, from what is at
/// each offset from it.
pub fn bookshelf_count(
    provider: impl Fn([i32; 3]) -> bool,
    transmitter: impl Fn([i32; 3]) -> bool,
) -> i32 {
    bookshelf_offsets()
        .filter(|&offset| is_valid_bookshelf(offset, &provider, &transmitter))
        .count() as i32
}

/// `EnchantmentHelper.getEnchantmentCost`: the level a row offers, from up
/// to 15 bookshelves.
pub fn enchantment_cost(random: &mut LegacyRandom, slot: i32, bookshelves: i32) -> i32 {
    let shelves = bookshelves.min(15);
    let selected =
        random.next_int(8) as i32 + 1 + (shelves >> 1) + random.next_int(shelves as u32 + 1) as i32;
    match slot {
        0 => (selected / 3).max(1),
        1 => selected * 2 / 3 + 1,
        _ => selected.max(shelves * 2),
    }
}

/// `RandomSource.setSeed` with an int seed, as Java widens it.
fn seeded(seed: i32) -> LegacyRandom {
    LegacyRandom::new(i64::from(seed) as u64)
}

/// `ItemStack.get(ENCHANTABLE)`: the stack's own component, else its
/// item's; none when the patch removes it.
fn enchantable_of(table: &dyn EnchantingTable, stack: &ItemStack) -> Option<i32> {
    let patch = stack.components.as_ref();
    if patch.is_some_and(|patch| patch.get("!minecraft:enchantable").is_some()) {
        return None;
    }
    match patch.and_then(|patch| patch.get("minecraft:enchantable")) {
        Some(component) => component
            .get("value")
            .and_then(Value::as_i64)
            .map(|value| value as i32),
        None => table.enchantable(&stack.id),
    }
}

/// `ItemStack.isEnchantable`: it has `enchantable` and its `enchantments`
/// are empty.
fn is_enchantable(table: &dyn EnchantingTable, stack: &ItemStack) -> bool {
    let patch = stack.components.as_ref();
    let enchantments = patch.and_then(|patch| patch.get("minecraft:enchantments"));
    let removed = patch.is_some_and(|patch| patch.get("!minecraft:enchantments").is_some())
        || enchantments.is_some_and(Value::is_null);
    let empty = enchantments.is_none_or(|map| map.as_object().is_some_and(|map| map.is_empty()));
    enchantable_of(table, stack).is_some() && !removed && empty
}

/// `ItemStack.enchant` (`EnchantmentHelper.updateEnchantments` with
/// `ItemEnchantments.Mutable.upgrade`): the level raised to at least
/// `level`, at most 255, in `stored_enchantments` on an enchanted book.
fn enchant(stack: &mut ItemStack, id: &str, level: i32) {
    if level <= 0 {
        return;
    }
    let key = if stack.id == "minecraft:enchanted_book" {
        "minecraft:stored_enchantments"
    } else {
        "minecraft:enchantments"
    };
    let patch = stack
        .components
        .get_or_insert_with(|| Value::Object(Default::default()));
    let Some(patch) = patch.as_object_mut() else {
        return;
    };
    let map = patch
        .entry(key)
        .or_insert_with(|| Value::Object(Default::default()));
    if !map.is_object() {
        *map = Value::Object(Default::default());
    }
    if let Some(map) = map.as_object_mut() {
        let current = map.get(id).and_then(Value::as_i64).unwrap_or(0) as i32;
        map.insert(id.to_owned(), Value::from(current.max(level.min(255))));
    }
}

/// `EnchantmentMenu` (`enchantment`).
#[derive(Clone)]
pub struct EnchantmentMenu {
    slots: Vec<SlotDef>,
    items: OwnSlots,
    access: Option<TableAccess>,
    /// `random`, seeded before every use.
    random: LegacyRandom,
    /// `costs`, `enchantmentSeed`, `enchantClue` and `levelClue`.
    costs: [i32; 3],
    seed: i32,
    clues: [i32; 3],
    level_clues: [i32; 3],
}

impl EnchantmentMenu {
    pub fn new(mut items: Vec<Option<ItemStack>>) -> Self {
        let mut slots = vec![
            SlotDef::own(ITEM, 15, 47),
            SlotDef::own(LAPIS, 35, 47).with_icon(LAPIS_ICON),
        ];
        slots.extend(standard_inventory_slots(8, 84));
        items.resize(2, None);
        Self {
            slots,
            items: OwnSlots::new(items),
            access: None,
            random: LegacyRandom::new(0),
            costs: [0; 3],
            seed: 0,
            clues: [-1; 3],
            level_clues: [-1; 3],
        }
    }

    /// The table's access, on a server.
    pub fn set_access(&mut self, access: Option<TableAccess>) {
        self.access = access;
    }

    /// The bookshelves around the table, counted again.
    pub fn set_bookshelves(&mut self, bookshelves: i32) {
        if let Some(access) = self.access.as_mut() {
            access.bookshelves = bookshelves;
        }
    }

    /// `getEnchantmentList`: what row `slot` gives the item at its cost,
    /// from the seed and the row; a book loses one of several at random.
    fn enchantment_list(
        &mut self,
        table: &dyn EnchantingTable,
        stack: &ItemStack,
        slot: usize,
        cost: i32,
    ) -> Vec<(i32, i32)> {
        self.random = seeded(self.seed.wrapping_add(slot as i32));
        let Some(enchantable) = enchantable_of(table, stack) else {
            return Vec::new();
        };
        let mut list = table.select(&mut self.random, &stack.id, enchantable, cost);
        if stack.id == "minecraft:book" && list.len() > 1 {
            list.remove(self.random.next_int(list.len() as u32) as usize);
        }
        list
    }

    /// `slotsChanged` for the enchanting slots: the rows' costs and clues
    /// for the item, or none.
    fn update_offers(&mut self) {
        let stack = self.items.get(ITEM).cloned();
        let access = self.access.clone();
        let enchantable = match (&stack, &access) {
            (None, _) => false,
            // A client cannot tell, and `access.execute` does nothing there.
            (Some(_), None) => return,
            (Some(stack), Some(access)) => is_enchantable(access.table.as_ref(), stack),
        };
        let (Some(stack), Some(access), true) = (stack, access, enchantable) else {
            self.costs = [0; 3];
            self.clues = [-1; 3];
            self.level_clues = [-1; 3];
            return;
        };
        self.random = seeded(self.seed);
        for slot in 0..3 {
            let cost = enchantment_cost(&mut self.random, slot as i32, access.bookshelves);
            self.costs[slot] = if cost < slot as i32 + 1 { 0 } else { cost };
            self.clues[slot] = -1;
            self.level_clues[slot] = -1;
        }
        for slot in 0..3 {
            if self.costs[slot] <= 0 {
                continue;
            }
            let list = self.enchantment_list(access.table.as_ref(), &stack, slot, self.costs[slot]);
            if !list.is_empty() {
                let (id, level) = list[self.random.next_int(list.len() as u32) as usize];
                self.clues[slot] = id;
                self.level_clues[slot] = level;
            }
        }
    }
}

impl Menu for EnchantmentMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:enchantment"
    }

    fn slots(&self) -> &[SlotDef] {
        &self.slots
    }

    fn own(&self) -> &OwnSlots {
        &self.items
    }

    fn own_mut(&mut self) -> &mut OwnSlots {
        &mut self.items
    }

    /// The lapis slot takes only lapis lazuli.
    fn may_place(&self, _cx: &MenuContext, slot: usize, stack: &ItemStack) -> bool {
        slot != LAPIS || stack.id == LAPIS_LAZULI
    }

    /// The item slot holds one.
    fn max_stack(&self, _cx: &MenuContext, slot: usize, stack: &ItemStack) -> i32 {
        let max = if slot == ITEM {
            1
        } else {
            super::CONTAINER_MAX_STACK
        };
        max.min(i32::from(stack.max))
    }

    /// `EnchantmentMenu.quickMoveStack`: the table's slots into the
    /// inventory, last first; lapis to its slot; anything else, one of it
    /// into the empty item slot.
    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let mut stack = item(self, cx, slot)?.clone();
        let clicked = stack.clone();
        if slot == ITEM || slot == LAPIS {
            if !move_item_stack_to(self, cx, &mut stack, INVENTORY, END, true) {
                return None;
            }
        } else if stack.id == LAPIS_LAZULI {
            if !move_item_stack_to(self, cx, &mut stack, LAPIS, LAPIS + 1, true) {
                return None;
            }
        } else {
            if item(self, cx, ITEM).is_some() || !self.may_place(cx, ITEM, &stack) {
                return None;
            }
            let single = ItemStack {
                count: 1,
                ..stack.clone()
            };
            stack.count -= 1;
            set(self, cx, ITEM, Some(single));
        }
        let left = stack.clone();
        put_back(self, cx, slot, stack);
        if left.count == clicked.count {
            return None;
        }
        self.on_take(cx, slot, &left);
        Some(clicked)
    }

    fn slots_changed(&mut self, _cx: &mut MenuContext, _slot: usize) {
        self.update_offers();
    }

    /// `clickMenuButton`: row `id` enchants the item for `id + 1` lapis and
    /// `id + 1` levels, needing as many levels as it costs (creative needs
    /// neither). Only a server's menu enchants; a client's answers whether
    /// it would.
    fn click_button(&mut self, cx: &mut MenuContext, id: i32) -> bool {
        if !(0..3).contains(&id) {
            return false;
        }
        let row = id as usize;
        let price = id + 1;
        let lapis = self
            .items
            .get(LAPIS)
            .map_or(0, |stack| i32::from(stack.count));
        if lapis < price && !cx.creative {
            return false;
        }
        let cost = self.costs[row];
        let Some(stack) = self.items.get(ITEM).cloned() else {
            return false;
        };
        if cost <= 0 || ((cx.xp_level < price || cx.xp_level < cost) && !cx.creative) {
            return false;
        }
        let Some(access) = self.access.clone() else {
            return true;
        };
        let list = self.enchantment_list(access.table.as_ref(), &stack, row, cost);
        if list.is_empty() {
            return true;
        }
        // `Player.onEnchantmentPerformed`: the levels go (none below 0,
        // which the player takes as no progress either), and the seed is
        // drawn again.
        cx.xp_levels_spent += price;
        cx.xp_level = (cx.xp_level - price).max(0);
        cx.enchantment_seed = cx.random.next_i32();
        let mut enchanted = stack;
        if enchanted.id == "minecraft:book" {
            // `transmuteCopy(ENCHANTED_BOOK)`, which stacks to one.
            enchanted.id = "minecraft:enchanted_book".to_owned();
            enchanted.max = 1;
        }
        for (enchantment, level) in list {
            if let Some(enchantment) = access.table.enchantment(enchantment) {
                enchant(&mut enchanted, enchantment, level);
            }
        }
        self.items.set(ITEM, Some(enchanted));
        // `ItemStack.consume`: none in creative.
        if let Some(mut lapis) = self.items.get(LAPIS).cloned().filter(|_| !cx.creative) {
            lapis.count = lapis.count.saturating_sub(price as u8);
            self.items.set(LAPIS, (lapis.count > 0).then_some(lapis));
        }
        cx.inventory.record_custom("enchant_item");
        self.seed = cx.enchantment_seed;
        self.update_offers();
        let pitch = cx.random.next_float() * 0.1 + 0.9;
        cx.sounds
            .push(("minecraft:block.enchantment_table.use", 1.0, pitch));
        true
    }

    /// `removed`: the carried stack back, then on a server the item and the
    /// lapis (`clearContainer`).
    fn removed(&mut self, cx: &mut MenuContext) {
        return_carried(cx);
        if self.access.is_some() {
            clear_own_slots(self, cx, 0..2);
        }
    }

    fn data(&self) -> Vec<i32> {
        let mut data = self.costs.to_vec();
        data.push(self.seed);
        data.extend(self.clues);
        data.extend(self.level_clues);
        data
    }

    fn set_data(&mut self, id: usize, value: i32) {
        match id {
            0..SEED => self.costs[id] = value,
            SEED => self.seed = value,
            CLUES..LEVEL_CLUES => self.clues[id - CLUES] = value,
            LEVEL_CLUES..10 => self.level_clues[id - LEVEL_CLUES] = value,
            _ => {}
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ContainerInput, MenuInput, handle};
    use super::*;
    use crate::inventory::Inventory;
    use crate::statistics::CUSTOM;
    use serde_json::json;

    fn st(id: &str, count: u8) -> ItemStack {
        let mut stack = ItemStack::new(format!("minecraft:{id}"), count);
        if id == "diamond_sword" {
            stack.max = 1;
        }
        stack
    }

    /// Sharpness and smite (ids 0 and 1, exclusive) for swords, unbreaking
    /// (2) for anything; a book takes all three. It draws as vanilla's
    /// selection would: once per pick.
    struct Table;

    impl EnchantingTable for Table {
        fn enchantable(&self, item: &str) -> Option<i32> {
            match item {
                "minecraft:diamond_sword" => Some(10),
                "minecraft:book" => Some(1),
                _ => None,
            }
        }

        fn select(
            &self,
            random: &mut LegacyRandom,
            item: &str,
            _enchantable: i32,
            cost: i32,
        ) -> Vec<(i32, i32)> {
            let first = random.next_int(2) as i32;
            if item == "minecraft:book" {
                vec![(first, 1), (2, 3)]
            } else {
                vec![(first, 1 + cost / 10), (2, 2)]
            }
        }

        fn enchantment(&self, id: i32) -> Option<&str> {
            [
                "minecraft:sharpness",
                "minecraft:smite",
                "minecraft:unbreaking",
            ]
            .get(id as usize)
            .copied()
        }
    }

    fn table(bookshelves: i32, seed: i32, own: Vec<Option<ItemStack>>) -> EnchantmentMenu {
        let mut menu = EnchantmentMenu::new(own);
        menu.set_access(Some(TableAccess {
            table: Arc::new(Table),
            bookshelves,
        }));
        menu.set_data(SEED, seed);
        menu.update_offers();
        menu
    }

    struct Player {
        inventory: Inventory,
        random: LegacyRandom,
        level: i32,
        creative: bool,
    }

    impl Player {
        fn new(level: i32) -> Self {
            Self {
                inventory: Inventory::default(),
                random: LegacyRandom::new(7),
                level,
                creative: false,
            }
        }

        /// The inputs, and what they spent: levels, the new seed, sounds.
        fn run(&mut self, menu: &mut EnchantmentMenu, inputs: &[MenuInput]) -> (i32, i32, usize) {
            let mut cx = MenuContext::new(&mut self.inventory, &mut self.random);
            cx.xp_level = self.level;
            cx.creative = self.creative;
            cx.enchantment_seed = 99;
            for input in inputs {
                handle(menu, &mut cx, input);
            }
            self.level = cx.xp_level;
            (cx.xp_levels_spent, cx.enchantment_seed, cx.sounds.len())
        }
    }

    #[test]
    fn the_slots_and_icon_are_vanillas() {
        let menu = EnchantmentMenu::new(Vec::new());
        assert_eq!(menu.slots().len(), 38);
        assert_eq!(
            (menu.slots[0].x, menu.slots[0].y, menu.slots[0].icon),
            (15, 47, None)
        );
        assert_eq!(
            (menu.slots[1].x, menu.slots[1].y, menu.slots[1].icon),
            (35, 47, Some(LAPIS_ICON))
        );
        assert_eq!((menu.slots[2].x, menu.slots[2].y), (8, 84));
        assert_eq!(menu.data(), [0, 0, 0, 0, -1, -1, -1, -1, -1, -1]);
    }

    #[test]
    fn bookshelves_count_through_an_air_gap() {
        // 32 offsets, two rings of 16.
        assert_eq!(bookshelf_offsets().count(), 32);
        assert_eq!(bookshelf_offsets().next(), Some([-2, 0, -2]));
        let ring = |[x, _, z]: [i32; 3]| x.abs() == 2 || z.abs() == 2;
        let air = |_: [i32; 3]| true;
        assert_eq!(bookshelf_count(ring, air), 32);
        // A block in the gap at (1, 0, 0) hides the three shelves behind
        // it: (2, 0, -1), (2, 0, 0) and (2, 0, 1), whose halves it is.
        let blocked = |at: [i32; 3]| at != [1, 0, 0];
        assert_eq!(bookshelf_count(ring, blocked), 29);
        // At a corner's half only the corner: (2, 1, 2).
        let corner = |at: [i32; 3]| at != [1, 1, 1];
        assert_eq!(bookshelf_count(ring, corner), 31);
        // The table's own cell is no half; shelves one block out count not.
        let near = |[x, _, z]: [i32; 3]| x.abs() <= 1 && z.abs() <= 1;
        assert_eq!(bookshelf_count(near, air), 0);
        assert!(!is_valid_bookshelf([2, 0, 0], ring, |at| at != [1, 0, 0]));
    }

    #[test]
    fn costs_follow_the_seed_and_the_bookshelves() {
        // `getEnchantmentCost` with `java.util.Random(seed)`, worked in
        // Java: (seed, bookshelves, costs).
        let cases = [
            (0, 0, [2, 2, 6]),
            (0, 8, [5, 6, 16]),
            (0, 15, [8, 13, 30]),
            (0, 20, [8, 13, 30]),
            (1, 0, [2, 3, 0]),
            (1, 15, [4, 12, 30]),
            (12345, 0, [1, 6, 7]),
            (12345, 8, [3, 9, 16]),
            (12345, 15, [6, 20, 30]),
            (-1_234_567, 8, [3, 13, 16]),
            (-1_234_567, 15, [9, 15, 30]),
            (i32::MAX, 0, [1, 5, 7]),
            (i32::MAX, 15, [8, 19, 30]),
        ];
        for (seed, shelves, costs) in cases {
            let menu = table(shelves, seed, vec![Some(st("diamond_sword", 1))]);
            assert_eq!(
                menu.data()[..3],
                costs,
                "seed {seed}, {shelves} bookshelves"
            );
            assert_eq!(menu.data()[SEED], seed);
            // Each offered row has its clue.
            for (row, cost) in costs.iter().enumerate() {
                let (clue, level) = (menu.data()[CLUES + row], menu.data()[LEVEL_CLUES + row]);
                assert_eq!(*cost > 0, clue >= 0 && level > 0, "{clue} {level}");
            }
        }
        // Nothing for an item without `enchantable`, or one enchanted.
        let stick = table(15, 0, vec![Some(st("stick", 1))]);
        assert_eq!(stick.data()[..3], [0, 0, 0]);
        let mut enchanted = st("diamond_sword", 1);
        enchanted.components = Some(json!({"minecraft:enchantments": {"minecraft:smite": 1}}));
        assert_eq!(table(15, 0, vec![Some(enchanted)]).data()[..3], [0, 0, 0]);
        // An empty `enchantments` is as none.
        let mut cleared = st("diamond_sword", 1);
        cleared.components = Some(json!({"minecraft:enchantments": {}}));
        assert_eq!(table(15, 0, vec![Some(cleared)]).data()[..3], [8, 13, 30]);
    }

    #[test]
    fn a_row_enchants_for_its_lapis_and_levels() {
        let lapis = || Some(st("lapis_lazuli", 5));
        let mut menu = table(15, 12345, vec![Some(st("diamond_sword", 1)), lapis()]);
        assert_eq!(menu.data()[..3], [6, 20, 30]);
        // Too few levels for the third row, or too little lapis.
        let mut poor = Player::new(29);
        assert_eq!(poor.run(&mut menu, &[MenuInput::Button(2)]), (0, 99, 0));
        let mut cheap = table(
            15,
            12345,
            vec![Some(st("diamond_sword", 1)), Some(st("lapis_lazuli", 2))],
        );
        assert_eq!(
            Player::new(30).run(&mut cheap, &[MenuInput::Button(2)]),
            (0, 99, 0)
        );
        assert!(!cheap.click_button(
            &mut MenuContext::new(&mut Inventory::default(), &mut LegacyRandom::new(0)),
            3
        ));
        // The second row at level 25: 2 levels and 2 lapis, the clue's
        // enchantment among those applied, the seed drawn again, a sound.
        let clue = menu.data()[CLUES + 1];
        let mut player = Player::new(25);
        let (spent, seed, sounds) = player.run(&mut menu, &[MenuInput::Button(1)]);
        assert_eq!((spent, player.level, sounds), (2, 23, 1));
        assert_ne!(seed, 99);
        assert_eq!(menu.data()[SEED], seed);
        assert_eq!(menu.items.get(LAPIS).map(|s| s.count), Some(3));
        let sword = menu.items.get(ITEM).cloned().unwrap();
        let enchantments = sword.components.as_ref().unwrap()["minecraft:enchantments"].clone();
        assert_eq!(enchantments["minecraft:unbreaking"], 2);
        let name = Table.enchantment(clue).unwrap();
        assert!(enchantments.get(name).is_some(), "{enchantments}");
        assert_eq!(
            menu.data()[..3],
            [0, 0, 0],
            "an enchanted item is offered nothing"
        );
        assert_eq!(
            player.inventory.take_stat_events(),
            [(CUSTOM.to_owned(), "minecraft:enchant_item".to_owned(), 1)]
        );
    }

    #[test]
    fn creative_pays_no_lapis_and_books_become_enchanted_books() {
        let mut menu = table(15, 0, vec![Some(st("book", 1))]);
        assert!(menu.data()[0] > 0);
        let mut player = Player::new(0);
        player.creative = true;
        let (spent, _, _) = player.run(&mut menu, &[MenuInput::Button(0)]);
        assert_eq!((spent, player.level), (1, 0), "none below 0");
        let book = menu.items.get(ITEM).cloned().unwrap();
        assert_eq!(
            (book.id.as_str(), book.max),
            ("minecraft:enchanted_book", 1)
        );
        // Of the two picks one was dropped at random.
        let stored = book.components.unwrap()["minecraft:stored_enchantments"].clone();
        assert_eq!(stored.as_object().map(|map| map.len()), Some(1), "{stored}");
        assert!(menu.items.get(LAPIS).is_none());
    }

    #[test]
    fn a_client_checks_a_row_without_enchanting() {
        let mut menu = EnchantmentMenu::new(vec![
            Some(st("diamond_sword", 1)),
            Some(st("lapis_lazuli", 3)),
        ]);
        for (id, value) in [(0, 3), (1, 0), (2, 0)] {
            menu.set_data(id, value);
        }
        let mut inventory = Inventory::default();
        let mut random = LegacyRandom::new(0);
        let mut cx = MenuContext::new(&mut inventory, &mut random);
        cx.xp_level = 3;
        assert!(menu.click_button(&mut cx, 0));
        assert!(!menu.click_button(&mut cx, 1), "no offer");
        cx.xp_level = 2;
        assert!(!menu.click_button(&mut cx, 0));
        assert_eq!(cx.xp_levels_spent, 0);
        assert_eq!(menu.items.get(LAPIS).map(|s| s.count), Some(3));
    }

    #[test]
    fn shift_clicks_and_closing_return_the_items() {
        let mut menu = table(0, 0, Vec::new());
        let mut player = Player::new(0);
        player.inventory.slots[0] = Some(st("diamond_sword", 1));
        player.inventory.slots[1] = Some(st("lapis_lazuli", 10));
        player.inventory.slots[2] = Some(st("book", 4));
        let quick = |slot: i32| MenuInput::Click {
            slot,
            button: 0,
            kind: ContainerInput::QuickMove,
        };
        // The hotbar is 29-37: the lapis to its slot, one book to the item
        // slot, then the sword finds it full.
        player.run(&mut menu, &[quick(30), quick(31), quick(29)]);
        assert_eq!(menu.items.get(LAPIS).map(|s| s.count), Some(10));
        assert_eq!(
            menu.items.get(ITEM).map(|s| (s.id.as_str(), s.count)),
            Some(("minecraft:book", 1))
        );
        assert_eq!(player.inventory.slots[2].as_ref().map(|s| s.count), Some(3));
        assert!(player.inventory.slots[0].is_some());
        assert!(menu.data()[0] > 0);
        // The lapis back out, to the last free slot of the hotbar; then the
        // menu closes, and the book joins its stack.
        player.run(&mut menu, &[quick(1), MenuInput::Close]);
        assert!(menu.items.items().iter().all(Option::is_none));
        assert_eq!(
            player.inventory.slots[8].as_ref().map(|s| s.count),
            Some(10)
        );
        assert_eq!(player.inventory.slots[2].as_ref().map(|s| s.count), Some(4));
    }
}
