//! `ItemCombinerMenu` (26.3), the base of the anvil's and the smithing
//! table's menus: their inputs, then the result, then the player's slots
//! at (8, 84). What the two menus share of it is its shift-click.
use super::{Menu, MenuContext, item, move_item_stack_to, put_back};
use crate::inventory::ItemStack;

/// `ItemCombinerMenu.quickMoveStack` for a menu whose result is menu slot
/// `result`, its inputs before it: the result into the inventory, last
/// first; an input into the inventory; a stack of the inventory into the
/// inputs when `into_inputs` (`canMoveIntoInputSlots`) holds, and nowhere
/// else if they take none; otherwise between the main inventory and the
/// hotbar. Nothing moved when the count did not change; `onTake` otherwise.
pub(super) fn quick_move_stack<M: Menu + ?Sized>(
    menu: &mut M,
    cx: &mut MenuContext,
    slot: usize,
    result: usize,
    into_inputs: bool,
) -> Option<ItemStack> {
    let mut stack = item(menu, cx, slot)?.clone();
    let clicked = stack.clone();
    let (inventory, hotbar, end) = (result + 1, result + 28, result + 37);
    let moved = if slot == result {
        move_item_stack_to(menu, cx, &mut stack, inventory, end, true)
    } else if slot < result {
        move_item_stack_to(menu, cx, &mut stack, inventory, end, false)
    } else if into_inputs && slot < end {
        move_item_stack_to(menu, cx, &mut stack, 0, result, false)
    } else if slot < hotbar {
        move_item_stack_to(menu, cx, &mut stack, hotbar, end, false)
    } else if slot < end {
        move_item_stack_to(menu, cx, &mut stack, inventory, hotbar, false)
    } else {
        true
    };
    if !moved {
        return None;
    }
    let left = stack.clone();
    put_back(menu, cx, slot, stack);
    if left.count == clicked.count {
        return None;
    }
    menu.on_take(cx, slot, &left);
    Some(clicked)
}
