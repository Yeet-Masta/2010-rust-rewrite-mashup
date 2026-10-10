//! `CrafterMenu` (26.3), the crafter's menu, with its `CrafterSlot`s and the
//! `NonInteractiveResultSlot` that shows what the grid crafts. The grid and
//! the ten data values (the slots' states, then `triggered`) are the block
//! entity's, which the caller loads with the slots. The result is the
//! server's (`refreshRecipeResult`): it comes as a tenth own slot, read with
//! the grid, which no input takes from or puts into.
//!
//! A slot state change applies the block entity's rule
//! (`CrafterBlockEntity.setSlotState`: only an empty grid slot toggles) and
//! asks the block entity to do the same ([`BlockRequest::SlotState`]).
use super::{
    BlockRequest, ContainerInput, Menu, MenuContext, OwnSlots, SlotDef, item, move_item_stack_to,
    put_back, standard_inventory_slots,
};
use crate::inventory::ItemStack;

/// `CrafterBlockEntity.CONTAINER_SIZE`: the grid's slots.
pub const GRID: usize = 9;
/// `CrafterBlockEntity.DATA_TRIGGERED`.
pub const DATA_TRIGGERED: usize = 9;
/// The result preview's menu slot, after the player's.
pub const RESULT: usize = 45;
/// `INV_SLOT_START` and `USE_ROW_SLOT_END`.
const INVENTORY: usize = 9;
const END: usize = 45;

/// `CrafterMenu` (`crafter_3x3`).
#[derive(Clone, Debug)]
pub struct CrafterMenu {
    slots: Vec<SlotDef>,
    /// The grid, then the result.
    items: OwnSlots,
    /// `ContainerData`: each grid slot's state (1 disabled, 0 enabled), then
    /// `triggered`.
    data: [i32; 10],
}

impl CrafterMenu {
    /// The grid's 9 stacks, and the result as a tenth when there is one.
    pub fn new(mut items: Vec<Option<ItemStack>>) -> Self {
        // `addSlots`: the grid, index x + 3y at (26 + 18x, 17 + 18y), the
        // player's, then the result at (134, 35).
        let mut slots: Vec<SlotDef> = (0..GRID)
            .map(|index| {
                SlotDef::own(
                    index,
                    26 + (index % 3) as i32 * 18,
                    17 + (index / 3) as i32 * 18,
                )
            })
            .collect();
        slots.extend(standard_inventory_slots(8, 84));
        slots.push(SlotDef::own(GRID, 134, 35));
        items.resize(GRID + 1, None);
        Self {
            slots,
            items: OwnSlots::new(items),
            data: [0; 10],
        }
    }

    /// `isSlotDisabled`.
    pub fn is_slot_disabled(&self, slot: usize) -> bool {
        slot < GRID && self.data[slot] == 1
    }

    /// `isPowered`: data value 9, `triggered`.
    pub fn is_powered(&self) -> bool {
        self.data[DATA_TRIGGERED] == 1
    }

    /// `CrafterScreen.slotClicked`'s toggle, before the click goes: on an
    /// empty grid slot, a plain click enables a disabled slot and, with
    /// nothing carried, disables an enabled one; a hotbar key holding
    /// something enables a disabled slot. The new state, when it toggles.
    pub fn toggle_for_click(
        &self,
        cx: &MenuContext,
        slot: usize,
        button: i32,
        kind: ContainerInput,
    ) -> Option<bool> {
        if slot >= GRID || item(self, cx, slot).is_some() {
            return None;
        }
        let disabled = self.is_slot_disabled(slot);
        match kind {
            ContainerInput::Pickup if disabled => Some(true),
            ContainerInput::Pickup if cx.inventory.cursor.is_none() => Some(false),
            ContainerInput::Swap => {
                let held = usize::try_from(button)
                    .ok()
                    .and_then(|index| cx.inventory.slots.get(index))
                    .is_some_and(Option::is_some);
                (disabled && held).then_some(true)
            }
            _ => None,
        }
    }
}

impl Menu for CrafterMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:crafter_3x3"
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

    /// `CrafterSlot.mayPlace` (not while disabled) and
    /// `NonInteractiveResultSlot.mayPlace` (never).
    fn may_place(&self, _cx: &MenuContext, slot: usize, _stack: &ItemStack) -> bool {
        match slot {
            RESULT => false,
            _ => !self.is_slot_disabled(slot),
        }
    }

    /// `NonInteractiveResultSlot.mayPickup`.
    fn may_pickup(&self, _cx: &MenuContext, slot: usize) -> bool {
        slot != RESULT
    }

    /// `NonInteractiveResultSlot.remove` gives nothing.
    fn remove(&mut self, cx: &mut MenuContext, slot: usize, amount: i32) -> Option<ItemStack> {
        if slot == RESULT {
            return None;
        }
        super::remove_from(self, cx, slot, amount)
    }

    /// `CrafterMenu.quickMoveStack`: the grid into the inventory, last
    /// first; the inventory into the grid. Nothing moved when the count did
    /// not change; `onTake` otherwise.
    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let mut stack = item(self, cx, slot)?.clone();
        let clicked = stack.clone();
        let moved = if slot < GRID {
            move_item_stack_to(self, cx, &mut stack, INVENTORY, END, true)
        } else {
            move_item_stack_to(self, cx, &mut stack, 0, GRID, false)
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

    /// `CrafterBlockEntity.setSlotState`: an empty grid slot takes the
    /// state, and so does the block entity.
    fn set_slot_state(&mut self, cx: &mut MenuContext, slot: usize, enabled: bool) {
        if slot >= GRID || self.items.get(slot).is_some() {
            return;
        }
        self.data[slot] = i32::from(!enabled);
        cx.block_requests.push(BlockRequest::SlotState { slot, enabled });
    }

    fn data(&self) -> Vec<i32> {
        self.data.to_vec()
    }

    fn set_data(&mut self, id: usize, value: i32) {
        if let Some(data) = self.data.get_mut(id) {
            *data = value;
        }
    }

    /// The result shows a container of its own (`resultContainer`).
    fn own_container(&self, slot: usize) -> usize {
        usize::from(slot == RESULT)
    }

    /// `NonInteractiveResultSlot.isHighlightable`.
    fn is_highlightable(&self, slot: usize) -> bool {
        slot != RESULT
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
    use super::super::{MenuInput, handle};
    use super::*;
    use crate::inventory::Inventory;
    use crate::rng::LegacyRandom;

    fn st(id: &str, count: u8) -> ItemStack {
        let mut stack = ItemStack::new(format!("minecraft:{id}"), count);
        stack.max = 64;
        stack
    }

    fn run(menu: &mut CrafterMenu, inventory: &mut Inventory, inputs: &[MenuInput]) -> Vec<BlockRequest> {
        let mut random = LegacyRandom::new(0);
        let mut cx = MenuContext::new(inventory, &mut random);
        for input in inputs {
            handle(menu, &mut cx, input);
        }
        cx.block_requests
    }

    fn click(slot: i32, button: i32, kind: ContainerInput) -> MenuInput {
        MenuInput::Click { slot, button, kind }
    }

    #[test]
    fn the_slots_and_data_are_vanillas() {
        let menu = CrafterMenu::new(Vec::new());
        assert_eq!(menu.menu_type(), "minecraft:crafter_3x3");
        assert_eq!(menu.slots().len(), 46);
        assert_eq!(menu.own().len(), 10);
        let at: Vec<(i32, i32)> = [0, 2, 8, 9, 36, 45].iter().map(|&s| (menu.slots()[s].x, menu.slots()[s].y)).collect();
        assert_eq!(at, [(26, 17), (62, 17), (62, 53), (8, 84), (8, 142), (134, 35)]);
        assert_eq!(menu.data().len(), 10);
        assert!(!menu.is_highlightable(RESULT) && menu.is_highlightable(0));
    }

    #[test]
    fn only_empty_slots_toggle_and_disabled_ones_take_nothing() {
        let mut menu = CrafterMenu::new(vec![Some(st("oak_planks", 2))]);
        let mut inventory = Inventory::default();
        let requests = run(&mut menu, &mut inventory, &[MenuInput::SlotState { slot: 0, enabled: false }, MenuInput::SlotState { slot: 4, enabled: false }]);
        assert_eq!(requests, [BlockRequest::SlotState { slot: 4, enabled: false }], "slot 0 holds planks");
        assert!(!menu.is_slot_disabled(0) && menu.is_slot_disabled(4));
        inventory.cursor = Some(st("stick", 3));
        run(&mut menu, &mut inventory, &[click(4, 0, ContainerInput::Pickup)]);
        assert_eq!((menu.own().get(4), inventory.cursor.clone()), (None, Some(st("stick", 3))));
        // A shift-click from the inventory passes the disabled slot by.
        inventory.cursor = None;
        inventory.slots[9] = Some(st("stick", 64));
        inventory.slots[10] = Some(st("dirt", 5));
        run(&mut menu, &mut inventory, &[click(9, 0, ContainerInput::QuickMove), click(10, 0, ContainerInput::QuickMove)]);
        assert_eq!(menu.own().get(1), Some(&st("stick", 64)));
        assert_eq!((menu.own().get(2), menu.own().get(4)), (Some(&st("dirt", 5)), None));
        // Out of the grid, into the hotbar's last slot first.
        run(&mut menu, &mut inventory, &[click(2, 0, ContainerInput::QuickMove)]);
        assert_eq!(inventory.slots[8], Some(st("dirt", 5)));
    }

    #[test]
    fn the_result_is_shown_and_never_taken() {
        let mut menu = CrafterMenu::new(vec![Some(st("oak_log", 1)), None, None, None, None, None, None, None, None, Some(st("oak_planks", 4))]);
        let mut inventory = Inventory::default();
        run(&mut menu, &mut inventory, &[click(45, 0, ContainerInput::Pickup), click(45, 0, ContainerInput::QuickMove), click(45, 1, ContainerInput::Throw), click(45, 3, ContainerInput::Swap)]);
        assert_eq!(menu.own().get(9), Some(&st("oak_planks", 4)));
        assert_eq!((inventory.cursor.clone(), inventory.slots[3].clone()), (None, None));
        inventory.cursor = Some(st("oak_planks", 1));
        run(&mut menu, &mut inventory, &[click(45, 0, ContainerInput::Pickup), MenuInput::Drag { button: 0, slots: vec![45, 3] }]);
        assert_eq!(menu.own().get(9), Some(&st("oak_planks", 4)));
    }

    #[test]
    fn the_screen_toggles_empty_slots_as_vanilla_clicks() {
        let mut menu = CrafterMenu::new(vec![Some(st("stick", 1))]);
        menu.set_data(5, 1);
        let mut inventory = Inventory::default();
        let mut random = LegacyRandom::new(0);
        let cx = MenuContext::new(&mut inventory, &mut random);
        assert_eq!(menu.toggle_for_click(&cx, 0, 0, ContainerInput::Pickup), None, "it holds a stick");
        assert_eq!(menu.toggle_for_click(&cx, 1, 0, ContainerInput::Pickup), Some(false));
        assert_eq!(menu.toggle_for_click(&cx, 5, 1, ContainerInput::Pickup), Some(true));
        assert_eq!(menu.toggle_for_click(&cx, 5, 2, ContainerInput::Swap), None, "an empty hotbar slot");
        assert_eq!(menu.toggle_for_click(&cx, 12, 0, ContainerInput::Pickup), None, "not the grid");
        cx.inventory.slots[2] = Some(st("dirt", 1));
        assert_eq!(menu.toggle_for_click(&cx, 5, 2, ContainerInput::Swap), Some(true));
        assert_eq!(menu.toggle_for_click(&cx, 1, 2, ContainerInput::Swap), None);
        cx.inventory.cursor = Some(st("dirt", 1));
        assert_eq!(menu.toggle_for_click(&cx, 1, 0, ContainerInput::Pickup), None, "a carried stack goes in");
        assert_eq!(menu.toggle_for_click(&cx, 5, 0, ContainerInput::Pickup), Some(true));
    }
}
