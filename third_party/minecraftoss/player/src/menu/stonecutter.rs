//! `StonecutterMenu` (26.3): the input, the stonecutting recipes that take
//! it (`selectByInput`, in the recipes' order), the one the screen's
//! buttons select (the data value `selectedRecipeIndex`), and its result.
//! Each take uses one input and makes the result again; a shift-click
//! makes as many as the input allows.
//!
//! The recipes come from the recipe book on both sides, as vanilla's client
//! reads the synced ones. A server's menu ([`StonecutterMenu::set_at_block`])
//! plays the take's sound, once a tick, and returns the input as it closes.
//!
//! Not simulated: the recipe's unlock and the crafted-item statistics.
use super::{
    Menu, MenuContext, OwnSlots, SlotDef, clear_own_slots, item, move_item_stack_to, put_back,
    remove_from, return_carried, standard_inventory_slots,
};
use crate::inventory::ItemStack;

const INPUT: usize = 0;
/// The result's menu slot.
pub const RESULT: usize = 1;
/// The main inventory is 2-28, the hotbar 29-37.
const INVENTORY: usize = 2;
const HOTBAR: usize = 29;
const END: usize = 38;
/// `SoundEvents.UI_STONECUTTER_TAKE_RESULT`.
const TAKE_SOUND: &str = "minecraft:ui.stonecutter.take_result";

/// `StonecutterMenu` (`stonecutter`).
#[derive(Clone, Debug)]
pub struct StonecutterMenu {
    slots: Vec<SlotDef>,
    /// The input, then the result.
    items: OwnSlots,
    /// The menu is a server's, at its block.
    at_block: bool,
    /// The data value `selectedRecipeIndex`: -1 for none.
    selected: i32,
    /// `input`: the item the recipe list was made for.
    input: Option<String>,
    /// `lastSoundTime`.
    last_sound_time: i64,
}

impl StonecutterMenu {
    pub fn new(mut items: Vec<Option<ItemStack>>) -> Self {
        let mut slots = vec![SlotDef::own(INPUT, 20, 33), SlotDef::own(RESULT, 143, 33)];
        slots.extend(standard_inventory_slots(8, 84));
        items.resize(2, None);
        let input = items[INPUT].as_ref().map(|stack| stack.id.clone());
        Self {
            slots,
            items: OwnSlots::new(items),
            at_block: false,
            selected: -1,
            input,
            last_sound_time: i64::MIN,
        }
    }

    /// The menu is a server's, at its block (`ContainerLevelAccess.create`).
    pub fn set_at_block(&mut self, at_block: bool) {
        self.at_block = at_block;
    }

    /// `getSelectedRecipeIndex`.
    pub fn selected(&self) -> i32 {
        self.selected
    }

    /// `getVisibleRecipes`: the results of the recipes that take the input,
    /// in order; none without an input.
    pub fn recipes(&self, cx: &MenuContext) -> Vec<ItemStack> {
        self.items.get(INPUT).map_or_else(Vec::new, |input| {
            cx.inventory
                .recipes
                .stonecutting_recipes(input)
                .into_iter()
                .map(|(_, result)| result)
                .collect()
        })
    }

    /// `setupResultSlot`: the selected recipe's result, or none.
    fn setup_result(&mut self, cx: &MenuContext) {
        let result = usize::try_from(self.selected)
            .ok()
            .and_then(|index| self.recipes(cx).into_iter().nth(index));
        self.items.set(RESULT, result);
    }

    /// The result slot's `onTake`: one input goes and the result is made
    /// again; at the block, its sound, once a tick.
    fn take_result(&mut self, cx: &mut MenuContext) {
        if remove_from(self, cx, INPUT, 1).is_some() {
            self.setup_result(cx);
        }
        if self.at_block && self.last_sound_time != cx.game_time {
            cx.sounds.push((TAKE_SOUND, 1.0, 1.0));
            self.last_sound_time = cx.game_time;
        }
    }
}

impl Menu for StonecutterMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:stonecutter"
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

    fn may_place(&self, _cx: &MenuContext, slot: usize, _stack: &ItemStack) -> bool {
        slot != RESULT
    }

    /// `StonecutterMenu.quickMoveStack`: the result into the inventory,
    /// last first, what does not fit thrown; the input into the inventory;
    /// a stack some recipe takes into the input; otherwise between the main
    /// inventory and the hotbar.
    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let mut stack = item(self, cx, slot)?.clone();
        let clicked = stack.clone();
        let moved = match slot {
            RESULT => move_item_stack_to(self, cx, &mut stack, INVENTORY, END, true),
            INPUT => move_item_stack_to(self, cx, &mut stack, INVENTORY, END, false),
            _ if !cx.inventory.recipes.stonecutting_recipes(&stack).is_empty() => {
                move_item_stack_to(self, cx, &mut stack, INPUT, RESULT, false)
            }
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
        // The take made the result again: the rest of the old one drops.
        if slot == RESULT {
            cx.throw(left, false);
        }
        Some(clicked)
    }

    /// `ResultContainer.removeItem`: the whole result.
    fn remove(&mut self, cx: &mut MenuContext, slot: usize, amount: i32) -> Option<ItemStack> {
        if slot != RESULT {
            return remove_from(self, cx, slot, amount);
        }
        let result = self.items.get(RESULT).cloned();
        self.items.set(RESULT, None);
        result
    }

    fn on_take(&mut self, cx: &mut MenuContext, slot: usize, _taken: &ItemStack) {
        if slot == RESULT {
            self.take_result(cx);
        } else {
            super::set_changed(self, cx, slot);
        }
    }

    /// `slotsChanged`: another item in the input makes the recipe list
    /// again, with none selected and no result.
    fn slots_changed(&mut self, _cx: &mut MenuContext, slot: usize) {
        if slot != INPUT {
            return;
        }
        let input = self.items.get(INPUT).map(|stack| stack.id.clone());
        if input != self.input {
            self.input = input;
            self.selected = -1;
            self.items.set(RESULT, None);
        }
    }

    /// `clickMenuButton`: a recipe not yet selected is, with its result;
    /// any other id is taken but changes nothing.
    fn click_button(&mut self, cx: &mut MenuContext, id: i32) -> bool {
        if self.selected == id {
            return false;
        }
        if usize::try_from(id).is_ok_and(|index| index < self.recipes(cx).len()) {
            self.selected = id;
            self.setup_result(cx);
        }
        true
    }

    /// `removed`: the result goes, then on a server the input comes back
    /// (`clearContainer`).
    fn removed(&mut self, cx: &mut MenuContext) {
        return_carried(cx);
        self.items.set(RESULT, None);
        if self.at_block {
            clear_own_slots(self, cx, INPUT..RESULT);
        }
    }

    fn data(&self) -> Vec<i32> {
        vec![self.selected]
    }

    fn set_data(&mut self, id: usize, value: i32) {
        if id == 0 {
            self.selected = value;
        }
    }

    /// The result is not gathered by a double click.
    fn can_take_for_pick_all(&self, _carried: &ItemStack, slot: usize) -> bool {
        slot != RESULT
    }

    fn own_container(&self, slot: usize) -> usize {
        usize::from(slot == RESULT)
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
    use super::super::{ContainerInput, MenuInput, handle, set};
    use super::*;
    use crate::crafting::RecipeBook;
    use crate::inventory::Inventory;
    use crate::rng::LegacyRandom;
    use serde_json::json;
    use std::sync::Arc;

    fn book() -> Arc<RecipeBook> {
        let recipe = |name: &str, result: &str, count: u8| {
            (
                format!("data/minecraft/recipe/{name}.json"),
                json!({"type": "minecraft:stonecutting", "ingredient": "minecraft:andesite", "result": {"id": result, "count": count}}),
            )
        };
        Arc::new(RecipeBook::from_files(vec![
            recipe(
                "andesite_slab_from_andesite_stonecutting",
                "minecraft:andesite_slab",
                2,
            ),
            recipe(
                "andesite_stairs_from_andesite_stonecutting",
                "minecraft:andesite_stairs",
                1,
            ),
        ]))
    }

    #[test]
    fn a_selected_recipe_cuts_and_a_shift_click_cuts_all_the_input() {
        let mut inventory = Inventory::default();
        inventory.recipes = book();
        let mut random = LegacyRandom::new(1);
        let mut cx = MenuContext::new(&mut inventory, &mut random);
        cx.game_time = 100;
        let mut menu = StonecutterMenu::new(Vec::new());
        menu.set_at_block(true);
        set(
            &mut menu,
            &mut cx,
            INPUT,
            Some(ItemStack::new("minecraft:andesite", 3)),
        );
        assert_eq!(menu.recipes(&cx).len(), 2);
        assert!(menu.click_button(&mut cx, 0));
        assert!(!menu.click_button(&mut cx, 0), "already selected");
        assert!(menu.click_button(&mut cx, 7), "taken, though no recipe");
        assert_eq!(menu.selected(), 0);
        assert_eq!(
            menu.own().get(RESULT),
            Some(&ItemStack::new("minecraft:andesite_slab", 2))
        );
        let shift = MenuInput::Click {
            slot: RESULT as i32,
            button: 0,
            kind: ContainerInput::QuickMove,
        };
        handle(&mut menu, &mut cx, &shift);
        assert_eq!(
            cx.inventory.slots[8],
            Some(ItemStack::new("minecraft:andesite_slab", 6))
        );
        assert!(menu.own().items().iter().all(Option::is_none));
        assert_eq!(menu.selected(), -1, "the input ran out");
        assert_eq!(cx.sounds.len(), 1, "one take sound a tick");
    }
}
