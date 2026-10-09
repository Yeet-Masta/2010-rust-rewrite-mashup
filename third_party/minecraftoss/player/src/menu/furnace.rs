//! `AbstractFurnaceMenu` (26.3), the menu of furnaces, smokers and blast
//! furnaces, with its `FurnaceFuelSlot` and `FurnaceResultSlot`. The
//! ingredient, fuel and result are the block entity's, and so are the four
//! data values, which the caller loads ([`Menu::set_data`]) with the slots.
//! What smelts and what burns comes from the player's recipe book. Taking
//! the result asks the block entity for its experience
//! ([`BlockRequest::AwardUsedRecipes`]).
//!
//! Not ported: the recipe book's placement (`handlePlacement`).
use super::{
    BlockRequest, CONTAINER_MAX_STACK, Menu, MenuContext, OwnSlots, SlotDef, item,
    move_item_stack_to, put_back, remove_from, set_changed, standard_inventory_slots,
};
use crate::crafting::CookingKind;
use crate::inventory::ItemStack;

const INGREDIENT: usize = 0;
const FUEL: usize = 1;
const RESULT: usize = 2;
/// `INV_SLOT_START`, `USE_ROW_SLOT_START` and `USE_ROW_SLOT_END`: the main
/// inventory is 3-29, the hotbar 30-38.
const INVENTORY: usize = 3;
const HOTBAR: usize = 30;
const END: usize = 39;

/// `AbstractFurnaceBlockEntity.BURN_TIME_STANDARD`.
const BURN_TIME_STANDARD: i32 = 200;

/// `FurnaceMenu`, `SmokerMenu` or `BlastFurnaceMenu`.
#[derive(Clone, Debug)]
pub struct FurnaceMenu {
    kind: CookingKind,
    slots: Vec<SlotDef>,
    items: OwnSlots,
    /// `ContainerData`: `litTimeRemaining`, `litTotalTime`, `cookingTimer`
    /// and `cookingTotalTime`.
    data: [i32; 4],
    /// `FurnaceResultSlot.removeCount`: taken since the last payment.
    remove_count: i32,
}

impl FurnaceMenu {
    pub fn new(kind: CookingKind, mut items: Vec<Option<ItemStack>>) -> Self {
        let mut slots = vec![
            SlotDef::own(INGREDIENT, 56, 17),
            SlotDef::own(FUEL, 56, 53),
            SlotDef::own(RESULT, 116, 35),
        ];
        slots.extend(standard_inventory_slots(8, 84));
        if items.len() < 3 {
            items.resize(3, None);
        }
        Self {
            kind,
            slots,
            items: OwnSlots::new(items),
            data: [0; 4],
            remove_count: 0,
        }
    }

    pub fn kind(&self) -> CookingKind {
        self.kind
    }

    /// `canSmelt`: an ingredient of a recipe of the furnace's type (its
    /// `furnace_input`, `smoker_input` or `blast_furnace_input` set).
    fn can_smelt(&self, cx: &MenuContext, stack: &ItemStack) -> bool {
        cx.inventory.recipes.cooking_for(self.kind, stack).is_some()
    }

    /// `FurnaceResultSlot.checkTakeAchievements`: the taken count is
    /// crafted (`onCraftedBy`), and the block entity pays its experience.
    fn check_take_achievements(&mut self, cx: &mut MenuContext, carried: &ItemStack) {
        if self.remove_count > 0 {
            let mut crafted = carried.clone();
            crafted.count = self.remove_count.min(i32::from(u8::MAX)) as u8;
            cx.inventory.record_crafted(&crafted);
        }
        cx.block_requests.push(BlockRequest::AwardUsedRecipes);
        self.remove_count = 0;
    }
}

/// `isFuel`: the item has `cooking_fuel`.
fn is_fuel(cx: &MenuContext, stack: &ItemStack) -> bool {
    cx.inventory.recipes.is_fuel(&stack.id)
}

/// `FurnaceFuelSlot.isBucket`.
fn is_bucket(stack: &ItemStack) -> bool {
    stack.id == "minecraft:bucket"
}

/// `getLitProgress`: how much of the fuel is left, 0 to 1.
pub fn lit_progress(data: &[i32]) -> f32 {
    let (remaining, total) = (value(data, 0), value(data, 1));
    let total = if total == 0 { BURN_TIME_STANDARD } else { total };
    (remaining as f32 / total as f32).clamp(0.0, 1.0)
}

/// `getBurnProgress`: how far the ingredient has cooked, 0 to 1.
pub fn burn_progress(data: &[i32]) -> f32 {
    let (current, total) = (value(data, 2), value(data, 3));
    if total != 0 && current != 0 {
        (current as f32 / total as f32).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// `isLit`.
pub fn is_lit(data: &[i32]) -> bool {
    value(data, 0) > 0
}

fn value(data: &[i32], id: usize) -> i32 {
    data.get(id).copied().unwrap_or(0)
}

impl Menu for FurnaceMenu {
    fn menu_type(&self) -> &'static str {
        match self.kind {
            CookingKind::Furnace => "minecraft:furnace",
            CookingKind::BlastFurnace => "minecraft:blast_furnace",
            CookingKind::Smoker => "minecraft:smoker",
        }
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

    /// `FurnaceFuelSlot.mayPlace` (fuel or a bucket) and
    /// `FurnaceResultSlot.mayPlace` (nothing).
    fn may_place(&self, cx: &MenuContext, slot: usize, stack: &ItemStack) -> bool {
        match slot {
            FUEL => is_fuel(cx, stack) || is_bucket(stack),
            RESULT => false,
            _ => true,
        }
    }

    /// `FurnaceFuelSlot.getMaxStackSize`: one bucket.
    fn max_stack(&self, _cx: &MenuContext, slot: usize, stack: &ItemStack) -> i32 {
        if slot == FUEL && is_bucket(stack) {
            1
        } else {
            CONTAINER_MAX_STACK.min(i32::from(stack.max))
        }
    }

    /// `FurnaceResultSlot.remove` counts what it gives. A take that empties
    /// the ingredient is `removeItem`'s, not `setItem`'s.
    fn remove(&mut self, cx: &mut MenuContext, slot: usize, amount: i32) -> Option<ItemStack> {
        if slot == RESULT {
            let held = item(self, cx, slot).map_or(0, |stack| i32::from(stack.count));
            self.remove_count += amount.min(held);
        }
        let taken = remove_from(self, cx, slot, amount);
        if slot == INGREDIENT && taken.is_some() && item(self, cx, slot).is_none() {
            cx.block_requests.push(BlockRequest::Emptied(INGREDIENT));
        }
        taken
    }

    /// `FurnaceResultSlot.onTake`.
    fn on_take(&mut self, cx: &mut MenuContext, slot: usize, taken: &ItemStack) {
        if slot == RESULT {
            self.check_take_achievements(cx, taken);
        }
        set_changed(self, cx, slot);
    }

    /// `AbstractFurnaceMenu.quickMoveStack`: the result into the inventory,
    /// last first, paying as a take (`onQuickCraft`); the ingredient and
    /// fuel into the inventory, first first; from the inventory, what
    /// smelts to the ingredient, else fuel to the fuel slot, else between
    /// the main inventory and the hotbar.
    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let mut stack = item(self, cx, slot)?.clone();
        let clicked = stack.clone();
        let moved = match slot {
            RESULT => {
                let moved = move_item_stack_to(self, cx, &mut stack, INVENTORY, END, true);
                // `Slot.onQuickCraft(picked, original)`.
                let count = i32::from(clicked.count) - i32::from(stack.count);
                if moved && count > 0 {
                    self.remove_count += count;
                    self.check_take_achievements(cx, &clicked);
                }
                moved
            }
            INGREDIENT | FUEL => move_item_stack_to(self, cx, &mut stack, INVENTORY, END, false),
            _ if self.can_smelt(cx, &stack) => {
                move_item_stack_to(self, cx, &mut stack, INGREDIENT, FUEL, false)
            }
            _ if is_fuel(cx, &stack) => move_item_stack_to(self, cx, &mut stack, FUEL, RESULT, false),
            INVENTORY..HOTBAR => move_item_stack_to(self, cx, &mut stack, HOTBAR, END, false),
            HOTBAR..END => move_item_stack_to(self, cx, &mut stack, INVENTORY, HOTBAR, false),
            _ => true,
        };
        if !moved {
            return None;
        }
        let left = stack.clone();
        put_back(self, cx, slot, stack);
        if left.count == clicked.count {
            return None;
        }
        self.on_take(cx, slot, &left);
        Some(clicked)
    }

    fn data(&self) -> Vec<i32> {
        self.data.to_vec()
    }

    fn set_data(&mut self, id: usize, value: i32) {
        if let Some(data) = self.data.get_mut(id) {
            *data = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ContainerInput, MenuInput, handle};
    use super::*;
    use crate::crafting::RecipeBook;
    use crate::inventory::Inventory;
    use crate::rng::LegacyRandom;
    use crate::statistics::CRAFTED;
    use serde_json::json;
    use std::sync::Arc;

    fn st(id: &str, count: u8) -> ItemStack {
        let max = if id.ends_with("bucket") { 16 } else { 64 };
        ItemStack {
            id: format!("minecraft:{id}"),
            count,
            max,
            components: None,
        }
    }

    /// Iron ore smelts and blasts, logs that burn smelt to charcoal; the
    /// fuels are those of the book without a catalog (coal, logs...).
    fn book() -> Arc<RecipeBook> {
        let recipe = |name: &str, kind: &str, ingredient: &str, result: &str| {
            (
                format!("data/minecraft/recipe/{name}.json"),
                json!({"type": kind, "ingredient": ingredient, "result": {"id": result}, "experience": 0.7}),
            )
        };
        Arc::new(RecipeBook::from_files(vec![
            recipe("iron_ingot_from_smelting_iron_ore", "minecraft:smelting", "minecraft:iron_ore", "minecraft:iron_ingot"),
            recipe("iron_ingot_from_blasting_iron_ore", "minecraft:blasting", "minecraft:iron_ore", "minecraft:iron_ingot"),
            recipe("charcoal", "minecraft:smelting", "#minecraft:logs_that_burn", "minecraft:charcoal"),
            (
                "data/minecraft/tags/item/logs_that_burn.json".to_owned(),
                json!({"values": ["minecraft:oak_log"]}),
            ),
        ]))
    }

    struct Rig {
        inventory: Inventory,
        random: LegacyRandom,
    }

    fn rig(stacks: &[(usize, ItemStack)]) -> Rig {
        let mut inventory = Inventory::default();
        inventory.recipes = book();
        for (slot, stack) in stacks {
            inventory.slots[*slot] = Some(stack.clone());
        }
        Rig {
            inventory,
            random: LegacyRandom::new(0),
        }
    }

    fn run(menu: &mut FurnaceMenu, rig: &mut Rig, inputs: &[MenuInput]) -> Vec<BlockRequest> {
        let mut cx = MenuContext::new(&mut rig.inventory, &mut rig.random);
        for input in inputs {
            handle(menu, &mut cx, input);
        }
        cx.block_requests
    }

    fn click(slot: i32, button: i32, kind: ContainerInput) -> MenuInput {
        MenuInput::Click { slot, button, kind }
    }

    #[test]
    fn the_slots_and_types_are_vanillas() {
        let menu = FurnaceMenu::new(CookingKind::Smoker, Vec::new());
        assert_eq!(menu.menu_type(), "minecraft:smoker");
        assert_eq!(menu.own().len(), 3);
        let at: Vec<(i32, i32)> = menu.slots().iter().take(4).map(|s| (s.x, s.y)).collect();
        assert_eq!(at, [(56, 17), (56, 53), (116, 35), (8, 84)]);
        assert_eq!(menu.slots().len(), 39);
        assert_eq!((menu.slots()[30].x, menu.slots()[30].y), (8, 142), "the hotbar");
        assert_eq!(FurnaceMenu::new(CookingKind::BlastFurnace, Vec::new()).menu_type(), "minecraft:blast_furnace");
    }

    #[test]
    fn shift_clicks_send_what_smelts_in_and_fuel_to_the_fuel() {
        let mut menu = FurnaceMenu::new(CookingKind::Furnace, Vec::new());
        // Inventory 9 is menu slot 3, the hotbar's 0 is 30.
        let mut rig = rig(&[(9, st("iron_ore", 5)), (10, st("coal", 3)), (11, st("oak_log", 4)), (12, st("dirt", 7)), (0, st("bucket", 2))]);
        run(&mut menu, &mut rig, &[click(3, 0, ContainerInput::QuickMove), click(4, 0, ContainerInput::QuickMove)]);
        assert_eq!(menu.own().get(0), Some(&st("iron_ore", 5)));
        assert_eq!(menu.own().get(1), Some(&st("coal", 3)));
        // A log both smelts and burns: smelting wins, and the ore is in the way.
        run(&mut menu, &mut rig, &[click(5, 0, ContainerInput::QuickMove)]);
        assert_eq!(rig.inventory.slots[11], Some(st("oak_log", 4)));
        // What neither smelts nor burns goes to the hotbar; an empty bucket is
        // no fuel to a shift-click, so the hotbar's goes up to the inventory.
        run(&mut menu, &mut rig, &[click(6, 0, ContainerInput::QuickMove), click(30, 0, ContainerInput::QuickMove)]);
        assert_eq!(rig.inventory.slots[1], Some(st("dirt", 7)));
        assert_eq!(rig.inventory.slots[9], Some(st("bucket", 2)));
        // Out of the furnace, into the inventory first first.
        run(&mut menu, &mut rig, &[click(0, 0, ContainerInput::QuickMove)]);
        assert_eq!(rig.inventory.slots[10], Some(st("iron_ore", 5)));
        assert_eq!(menu.own().get(0), None);
    }

    #[test]
    fn the_fuel_slot_takes_fuel_and_one_bucket_and_the_result_nothing() {
        let mut menu = FurnaceMenu::new(CookingKind::Furnace, Vec::new());
        let mut rig = rig(&[]);
        rig.inventory.cursor = Some(st("bucket", 3));
        run(&mut menu, &mut rig, &[click(1, 0, ContainerInput::Pickup), click(2, 0, ContainerInput::Pickup)]);
        assert_eq!(menu.own().get(1), Some(&st("bucket", 1)));
        assert_eq!(menu.own().get(2), None);
        assert_eq!(rig.inventory.cursor, Some(st("bucket", 2)));
        let mut rig = self::rig(&[]);
        rig.inventory.cursor = Some(st("dirt", 3));
        let mut menu = FurnaceMenu::new(CookingKind::Furnace, Vec::new());
        run(&mut menu, &mut rig, &[click(1, 0, ContainerInput::Pickup)]);
        assert_eq!(menu.own().get(1), None, "dirt is no fuel");
    }

    #[test]
    fn taking_the_result_counts_it_and_asks_for_the_experience() {
        let mut menu = FurnaceMenu::new(CookingKind::Furnace, vec![None, None, Some(st("iron_ingot", 6))]);
        let mut rig = rig(&[]);
        let _ = rig.inventory.take_stat_events();
        let requests = run(&mut menu, &mut rig, &[click(2, 1, ContainerInput::Pickup)]);
        assert_eq!(rig.inventory.cursor, Some(st("iron_ingot", 3)), "a right click takes half");
        assert_eq!(requests, [BlockRequest::AwardUsedRecipes]);
        let crafted: Vec<_> = rig.inventory.take_stat_events().into_iter().filter(|(kind, _, _)| kind == CRAFTED).collect();
        assert_eq!(crafted, [(CRAFTED.to_owned(), "minecraft:iron_ingot".to_owned(), 3)]);
        // A shift-click pays through `onQuickCraft`, then again (for nothing)
        // through `onTake`.
        let requests = run(&mut menu, &mut rig, &[click(2, 0, ContainerInput::QuickMove)]);
        assert_eq!(rig.inventory.slots[8], Some(st("iron_ingot", 3)), "the hotbar's last slot first");
        assert_eq!(requests, [BlockRequest::AwardUsedRecipes, BlockRequest::AwardUsedRecipes]);
        let crafted: Vec<_> = rig.inventory.take_stat_events().into_iter().filter(|(kind, _, _)| kind == CRAFTED).collect();
        assert_eq!(crafted, [(CRAFTED.to_owned(), "minecraft:iron_ingot".to_owned(), 3)]);
    }

    #[test]
    fn emptying_the_ingredient_by_hand_is_a_removal() {
        let mut menu = FurnaceMenu::new(CookingKind::Furnace, vec![Some(st("iron_ore", 4)), None, None]);
        let mut rig = rig(&[]);
        assert_eq!(run(&mut menu, &mut rig, &[click(0, 1, ContainerInput::Pickup)]), [], "half leaves some");
        assert_eq!(run(&mut menu, &mut rig, &[click(-999, 0, ContainerInput::Pickup), click(0, 0, ContainerInput::Pickup)]), [BlockRequest::Emptied(0)]);
        assert_eq!(menu.own().get(0), None);
    }

    #[test]
    fn progress_reads_the_data_as_the_screen_does() {
        assert_eq!(lit_progress(&[800, 1600, 0, 0]), 0.5);
        assert_eq!(lit_progress(&[100, 0, 0, 0]), 0.5, "no total reads as 200");
        assert_eq!(burn_progress(&[0, 0, 50, 200]), 0.25);
        assert_eq!(burn_progress(&[0, 0, 50, 0]), 0.0);
        assert!(is_lit(&[1, 0, 0, 0]) && !is_lit(&[0, 1600, 0, 0]));
        let mut menu = FurnaceMenu::new(CookingKind::Furnace, Vec::new());
        menu.set_data(3, 200);
        menu.set_data(9, 1);
        assert_eq!(menu.data(), [0, 0, 0, 200]);
    }
}
