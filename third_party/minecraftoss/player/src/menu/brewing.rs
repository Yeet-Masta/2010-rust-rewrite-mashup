//! `BrewingStandMenu` (26.3), with its `PotionSlot`s, `IngredientsSlot` and
//! `FuelSlot`. The bottles, reagent and fuel are the block entity's, and so
//! are the four data values (the brewing time, the fuel, and their totals),
//! which the caller loads ([`Menu::set_data`]) with the slots. What the
//! slots take comes from the player's recipe book: the brewing recipes'
//! bottles and reagents, and the items with `brewing_fuel`.
//!
//! Not simulated: the `brewed_potion` trigger of a bottle taken.
use super::{
    CONTAINER_MAX_STACK, Menu, MenuContext, OwnSlots, SlotDef, item, move_item_stack_to, put_back,
    standard_inventory_slots, write,
};
use crate::inventory::ItemStack;

/// The bottles are 0-2.
const BOTTLES: usize = 3;
const INGREDIENT: usize = 3;
const FUEL: usize = 4;
/// `INV_SLOT_START`, `USE_ROW_SLOT_START` and `USE_ROW_SLOT_END`: the main
/// inventory is 5-31, the hotbar 32-40.
const INVENTORY: usize = 5;
const HOTBAR: usize = 32;
const END: usize = 41;

/// `EMPTY_SLOT_POTION` and `EMPTY_SLOT_FUEL`.
pub const POTION_ICON: &str = "container/slot/potion";
pub const FUEL_ICON: &str = "container/slot/brewing_fuel";

/// `BrewingStandScreen.BUBBLELENGTHS`.
pub const BUBBLE_LENGTHS: [i32; 7] = [29, 24, 20, 16, 11, 6, 0];

/// `BrewingStandMenu` (`brewing_stand`).
#[derive(Clone, Debug)]
pub struct BrewingStandMenu {
    slots: Vec<SlotDef>,
    items: OwnSlots,
    /// `ContainerData`: `brewTime`, `fuel`, `totalBrewTime` and `totalFuel`.
    data: [i32; 4],
}

impl BrewingStandMenu {
    pub fn new(mut items: Vec<Option<ItemStack>>) -> Self {
        let mut slots = vec![
            SlotDef::own(0, 56, 51).with_icon(POTION_ICON),
            SlotDef::own(1, 79, 58).with_icon(POTION_ICON),
            SlotDef::own(2, 102, 51).with_icon(POTION_ICON),
            SlotDef::own(INGREDIENT, 79, 17),
            SlotDef::own(FUEL, 17, 17).with_icon(FUEL_ICON),
        ];
        slots.extend(standard_inventory_slots(8, 84));
        if items.len() < 5 {
            items.resize(5, None);
        }
        Self {
            slots,
            items: OwnSlots::new(items),
            data: [0; 4],
        }
    }
}

/// `FuelSlot.mayPlaceItem`: the item has `brewing_fuel`.
fn is_fuel(cx: &MenuContext, stack: &ItemStack) -> bool {
    cx.inventory.recipes.brewing_fuel(&stack.id).is_some()
}

/// `PotionIngredient.isPotionInput`.
fn is_potion_input(cx: &MenuContext, stack: &ItemStack) -> bool {
    cx.inventory.recipes.is_potion_input(stack)
}

/// The fuel bar's length, 0 to 18: `ceilDiv(18 * fuel, totalFuel)`.
pub fn fuel_length(data: &[i32]) -> i32 {
    let (fuel, total) = (value(data, 1), value(data, 3));
    if total <= 0 {
        return 0;
    }
    // `Mth.positiveCeilDiv`.
    (-(-18 * fuel).div_euclid(total)).clamp(0, 18)
}

/// While brewing, the arrow's length, 0 to 28 (`28 * (1 - ticks /
/// total)`), and the bubbles' (`BUBBLELENGTHS[ticks / 2 % 7]`).
pub fn brew_progress(data: &[i32]) -> Option<(i32, i32)> {
    let (ticks, total) = (value(data, 0), value(data, 2));
    if ticks <= 0 || total <= 0 {
        return None;
    }
    let arrow = (28.0 * (1.0 - ticks as f32 / total as f32)) as i32;
    Some((arrow, BUBBLE_LENGTHS[(ticks / 2 % 7) as usize]))
}

fn value(data: &[i32], id: usize) -> i32 {
    data.get(id).copied().unwrap_or(0)
}

impl Menu for BrewingStandMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:brewing_stand"
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

    /// `PotionSlot.mayPlace` (a potion input), `IngredientsSlot.mayPlace`
    /// (a reagent) and `FuelSlot.mayPlace` (fuel).
    fn may_place(&self, cx: &MenuContext, slot: usize, stack: &ItemStack) -> bool {
        match slot {
            0..BOTTLES => is_potion_input(cx, stack),
            INGREDIENT => cx.inventory.recipes.is_brewing_reagent(stack),
            FUEL => is_fuel(cx, stack),
            _ => true,
        }
    }

    /// `PotionSlot.getMaxStackSize`: one bottle each.
    fn max_stack(&self, _cx: &MenuContext, slot: usize, stack: &ItemStack) -> i32 {
        if slot < BOTTLES {
            1.min(i32::from(stack.max))
        } else {
            CONTAINER_MAX_STACK.min(i32::from(stack.max))
        }
    }

    /// `BrewingStandMenu.quickMoveStack`: the stand's slots into the
    /// inventory, last first; from the inventory, fuel to the fuel slot (and
    /// that is all, whatever is left), else a reagent to the ingredient, a
    /// potion input to the bottles, or between the main inventory and the
    /// hotbar. Nothing moved when the count did not change; `onTake`
    /// otherwise.
    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let mut stack = item(self, cx, slot)?.clone();
        let clicked = stack.clone();
        let to = |menu: &mut Self, cx: &mut MenuContext, stack: &mut ItemStack, start: usize, end: usize| {
            move_item_stack_to(menu, cx, stack, start, end, false)
        };
        if slot >= INVENTORY {
            if is_fuel(cx, &clicked) {
                if to(self, cx, &mut stack, FUEL, FUEL + 1) {
                    // The method ends here: the slot keeps what is left,
                    // and no more is moved.
                    write(self, cx, slot, (stack.count > 0).then_some(stack));
                    return None;
                }
                if self.may_place(cx, INGREDIENT, &stack)
                    && !to(self, cx, &mut stack, INGREDIENT, FUEL)
                {
                    return None;
                }
            } else if self.may_place(cx, INGREDIENT, &stack) {
                if !to(self, cx, &mut stack, INGREDIENT, FUEL) {
                    return None;
                }
            } else if is_potion_input(cx, &clicked) {
                if !to(self, cx, &mut stack, 0, BOTTLES) {
                    return None;
                }
            } else if (INVENTORY..HOTBAR).contains(&slot) {
                if !to(self, cx, &mut stack, HOTBAR, END) {
                    return None;
                }
            } else if (HOTBAR..END).contains(&slot) {
                if !to(self, cx, &mut stack, INVENTORY, HOTBAR) {
                    return None;
                }
            } else if !to(self, cx, &mut stack, INVENTORY, END) {
                return None;
            }
        } else if !move_item_stack_to(self, cx, &mut stack, INVENTORY, END, true) {
            return None;
        }
        let left = stack.count;
        put_back(self, cx, slot, stack);
        if left == clicked.count {
            return None;
        }
        self.on_take(cx, slot, &clicked);
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
    use serde_json::json;
    use std::sync::Arc;

    fn st(id: &str, count: u8) -> ItemStack {
        ItemStack::new(format!("minecraft:{id}"), count)
    }

    fn potion(name: &str) -> ItemStack {
        let mut stack = st("potion", 1);
        stack.max = 1;
        stack.components = Some(json!({"minecraft:potion_contents": {"potion": format!("minecraft:{name}")}}));
        stack
    }

    /// Water and nether wart make awkward potions, awkward ones and sugar
    /// swiftness; the bottles' tag holds glass bottles too.
    fn book() -> Arc<RecipeBook> {
        let recipe = |from: &str, reagent: &str, to: &str| {
            (
                format!("data/minecraft/recipe/brewing/potion_{from}_{reagent}.json"),
                json!({
                    "type": "minecraft:brewing",
                    "input": {"item": "minecraft:potion", "potion_contents": {"potions": format!("minecraft:{from}")}},
                    "reagent": {"item": format!("minecraft:{reagent}")},
                    "output": {"id": "minecraft:potion", "components": {"minecraft:potion_contents": {"potion": format!("minecraft:{to}")}}}
                }),
            )
        };
        Arc::new(RecipeBook::from_files(vec![
            recipe("water", "nether_wart", "awkward"),
            recipe("awkward", "sugar", "swiftness"),
            recipe("awkward", "blaze_powder", "strength"),
            (
                "data/minecraft/tags/item/brewing_potion_inputs.json".to_owned(),
                json!({"values": ["minecraft:potion", "minecraft:glass_bottle"]}),
            ),
        ]))
    }

    fn inventory(stacks: &[(usize, ItemStack)]) -> Inventory {
        let mut inventory = Inventory::default();
        inventory.recipes = book();
        for (slot, stack) in stacks {
            inventory.slots[*slot] = Some(stack.clone());
        }
        inventory
    }

    fn run(menu: &mut BrewingStandMenu, inventory: &mut Inventory, inputs: &[MenuInput]) {
        let mut random = LegacyRandom::new(0);
        let mut cx = MenuContext::new(inventory, &mut random);
        for input in inputs {
            handle(menu, &mut cx, input);
        }
    }

    fn click(slot: i32, kind: ContainerInput) -> MenuInput {
        MenuInput::Click { slot, button: 0, kind }
    }

    #[test]
    fn the_slots_and_icons_are_vanillas() {
        let menu = BrewingStandMenu::new(Vec::new());
        assert_eq!(menu.menu_type(), "minecraft:brewing_stand");
        assert_eq!((menu.own().len(), menu.slots().len()), (5, 41));
        let at: Vec<(i32, i32, Option<&str>)> = menu.slots()[..6].iter().map(|s| (s.x, s.y, s.icon)).collect();
        assert_eq!(at, [(56, 51, Some(POTION_ICON)), (79, 58, Some(POTION_ICON)), (102, 51, Some(POTION_ICON)), (79, 17, None), (17, 17, Some(FUEL_ICON)), (8, 84, None)]);
        assert_eq!((menu.slots()[32].x, menu.slots()[32].y), (8, 142), "the hotbar");
    }

    #[test]
    fn shift_clicks_send_fuel_reagents_and_bottles_to_their_slots() {
        let mut menu = BrewingStandMenu::new(vec![None, None, None, None, Some(st("blaze_powder", 60))]);
        // Inventory 9 is menu slot 5, the hotbar's 0 is 32.
        let mut inventory = inventory(&[(9, st("blaze_powder", 10)), (10, st("sugar", 3)), (11, potion("water")), (12, potion("water")), (13, st("glass_bottle", 2)), (14, potion("water")), (15, st("dirt", 4))]);
        run(&mut menu, &mut inventory, &[5, 6, 7, 8, 9, 10, 11].map(|slot| click(slot, ContainerInput::QuickMove)));
        assert_eq!(menu.own().get(4), Some(&st("blaze_powder", 64)));
        assert_eq!(inventory.slots[9], Some(st("blaze_powder", 6)), "what fills the fuel ends the shift-click");
        assert_eq!(menu.own().get(3), Some(&st("sugar", 3)));
        assert_eq!(menu.own().items()[..3], [Some(potion("water")), Some(potion("water")), Some(st("glass_bottle", 1))], "one to a bottle slot");
        assert_eq!((inventory.slots[13].clone(), inventory.slots[14].clone()), (Some(st("glass_bottle", 1)), Some(potion("water"))), "no bottle slot left");
        assert_eq!(inventory.slots[0], Some(st("dirt", 4)), "the rest goes to the hotbar");
        // With the fuel full, blaze powder is a reagent: it waits on the
        // sugar.
        run(&mut menu, &mut inventory, &[click(5, ContainerInput::QuickMove)]);
        assert_eq!(inventory.slots[9], Some(st("blaze_powder", 6)));
        run(&mut menu, &mut inventory, &[click(3, ContainerInput::QuickMove), click(5, ContainerInput::QuickMove)]);
        assert_eq!(menu.own().get(3), Some(&st("blaze_powder", 6)));
        assert_eq!(inventory.slots[8], Some(st("sugar", 3)), "out of the stand, the hotbar's last slot first");
    }

    #[test]
    fn the_slots_take_only_their_items() {
        let mut menu = BrewingStandMenu::new(Vec::new());
        let mut inventory = inventory(&[]);
        inventory.cursor = Some(st("glass_bottle", 5));
        run(&mut menu, &mut inventory, &[click(0, ContainerInput::Pickup), click(3, ContainerInput::Pickup), click(4, ContainerInput::Pickup)]);
        assert_eq!(menu.own().items(), [Some(st("glass_bottle", 1)), None, None, None, None]);
        inventory.cursor = Some(st("sugar", 5));
        run(&mut menu, &mut inventory, &[click(1, ContainerInput::Pickup), click(4, ContainerInput::Pickup), click(3, ContainerInput::Pickup)]);
        assert_eq!(menu.own().items(), [Some(st("glass_bottle", 1)), None, None, Some(st("sugar", 5)), None]);
    }

    #[test]
    fn the_screen_reads_the_data_as_vanilla() {
        assert_eq!(fuel_length(&[0, 10, 400, 20]), 9);
        assert_eq!(brew_progress(&[100, 10, 400, 20]), Some((21, 24)), "50 % 7 is 1");
    }
}
