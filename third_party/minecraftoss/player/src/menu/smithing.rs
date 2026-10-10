//! `SmithingMenu` (26.3) on `ItemCombinerMenu`: a template, a base and an
//! addition, each slot taking what some smithing recipe's ingredient does
//! (the `smithing_*` recipe property sets), and the result of the first
//! recipe they match (`smithing_transform` keeps the base's components on
//! its new item, `smithing_trim` sets the `trim`). Taking it uses one of
//! each input.
//!
//! Only a server's menu ([`SmithingMenu::set_at_block`]) finds recipes, as
//! vanilla's client leaves its result empty for the server's, and only it
//! sets the data value `hasRecipeError`: the three inputs make nothing.
//!
//! Not simulated: the recipe's unlock and the crafted-item statistics.
use super::{
    Menu, MenuContext, OwnSlots, SlotDef, clear_own_slots, combiner, item, return_carried, set,
    set_changed, standard_inventory_slots,
};
use crate::inventory::ItemStack;

const TEMPLATE: usize = 0;
const BASE: usize = 1;
const ADDITION: usize = 2;
/// The result's menu slot. The main inventory is 4-30, the hotbar 31-39.
pub const RESULT: usize = 3;
/// `LevelEvent.SOUND_SMITHING_TABLE_USED`.
pub const SMITHING_TABLE_USED: i32 = 1044;

/// `SmithingMenu` (`smithing`).
#[derive(Clone, Debug)]
pub struct SmithingMenu {
    slots: Vec<SlotDef>,
    /// The template, the base, the addition, then the result.
    items: OwnSlots,
    /// The menu is a server's, at its block.
    at_block: bool,
    /// The data value `hasRecipeError`.
    recipe_error: i32,
}

impl SmithingMenu {
    pub fn new(mut items: Vec<Option<ItemStack>>) -> Self {
        let mut slots = vec![
            SlotDef::own(TEMPLATE, 8, 48),
            SlotDef::own(BASE, 26, 48),
            SlotDef::own(ADDITION, 44, 48),
            SlotDef::own(RESULT, 98, 48),
        ];
        slots.extend(standard_inventory_slots(8, 84));
        items.resize(4, None);
        Self {
            slots,
            items: OwnSlots::new(items),
            at_block: false,
            recipe_error: 0,
        }
    }

    /// The menu is a server's, at its block (`ContainerLevelAccess.create`).
    pub fn set_at_block(&mut self, at_block: bool) {
        self.at_block = at_block;
    }

    /// `hasRecipeError`.
    pub fn has_recipe_error(&self) -> bool {
        self.recipe_error > 0
    }

    /// The input `slot`'s recipe property set holds the stack.
    fn takes(cx: &MenuContext, slot: usize, stack: &ItemStack) -> bool {
        let recipes = &cx.inventory.recipes;
        match slot {
            TEMPLATE => recipes.is_smithing_template(stack),
            BASE => recipes.is_smithing_base(stack),
            ADDITION => recipes.is_smithing_addition(stack),
            _ => false,
        }
    }

    /// `canMoveIntoInputSlots`: an empty input takes the stack.
    fn can_move_into_inputs(&self, cx: &MenuContext, stack: &ItemStack) -> bool {
        (TEMPLATE..RESULT)
            .any(|slot| self.items.get(slot).is_none() && Self::takes(cx, slot, stack))
    }

    /// `createResult`: on a server, the first smithing recipe the inputs
    /// match, assembled; on a client, nothing.
    fn create_result(&mut self, cx: &MenuContext) {
        let result = if self.at_block {
            let [template, base, addition] =
                [TEMPLATE, BASE, ADDITION].map(|slot| self.items.get(slot));
            cx.inventory
                .recipes
                .smithing_result(template, base, addition)
                .map(|(_, result)| result)
        } else {
            None
        };
        self.items.set(RESULT, result);
    }
}

impl Menu for SmithingMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:smithing"
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

    fn may_place(&self, cx: &MenuContext, slot: usize, stack: &ItemStack) -> bool {
        Self::takes(cx, slot, stack)
    }

    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let into = item(self, cx, slot).is_some_and(|stack| self.can_move_into_inputs(cx, stack));
        combiner::quick_move_stack(self, cx, slot, RESULT, into)
    }

    /// `ResultContainer.removeItem`: the whole result.
    fn remove(&mut self, cx: &mut MenuContext, slot: usize, amount: i32) -> Option<ItemStack> {
        if slot != RESULT {
            return super::remove_from(self, cx, slot, amount);
        }
        let result = self.items.get(RESULT).cloned();
        self.items.set(RESULT, None);
        result
    }

    /// `SmithingMenu.onTake`: one of each input goes (`shrinkStackInSlot`),
    /// and the table sounds at its block.
    fn on_take(&mut self, cx: &mut MenuContext, slot: usize, _taken: &ItemStack) {
        if slot != RESULT {
            set_changed(self, cx, slot);
            return;
        }
        for input in TEMPLATE..RESULT {
            if let Some(stack) = self.items.get(input).cloned() {
                let rest = ItemStack {
                    count: stack.count - 1,
                    ..stack
                };
                set(self, cx, input, (rest.count > 0).then_some(rest));
            }
        }
        if self.at_block {
            cx.level_events.push(SMITHING_TABLE_USED);
        }
    }

    /// The inputs changed: `createResult`, then on a server whether three
    /// inputs make nothing.
    fn slots_changed(&mut self, cx: &mut MenuContext, slot: usize) {
        if slot >= RESULT {
            return;
        }
        self.create_result(cx);
        if self.at_block {
            let full =
                (TEMPLATE..=RESULT).all(|slot| self.items.get(slot).is_some() == (slot < RESULT));
            self.recipe_error = i32::from(full);
        }
    }

    /// `ItemCombinerMenu.removed`: the carried stack back, then on a server
    /// the inputs (`clearContainer`).
    fn removed(&mut self, cx: &mut MenuContext) {
        return_carried(cx);
        if self.at_block {
            clear_own_slots(self, cx, TEMPLATE..RESULT);
        }
    }

    fn data(&self) -> Vec<i32> {
        vec![self.recipe_error]
    }

    fn set_data(&mut self, id: usize, value: i32) {
        if id == 0 {
            self.recipe_error = value;
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
