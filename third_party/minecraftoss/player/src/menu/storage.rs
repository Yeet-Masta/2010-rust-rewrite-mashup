//! The storage menus: blocks whose items stay in them when the menu closes.
//! `ChestMenu` (chests, trapped chests, barrels, ender chests and double
//! chests), `ShulkerBoxMenu`, `HopperMenu` and `DispenserMenu` (dispensers
//! and droppers), with vanilla's slot positions. Each owns a copy of the
//! block's items; the caller loads it and writes back the slots that changed.
use super::{
    Menu, MenuContext, OwnSlots, SlotDef, container_quick_move, item, move_item_stack_to, put_back,
    standard_inventory_slots,
};
use crate::inventory::ItemStack;

/// The block's items, padded to the `size` the menu shows
/// (`checkContainerSize` refuses a smaller container). A larger container
/// keeps its extra slots, unshown.
fn container(mut items: Vec<Option<ItemStack>>, size: usize) -> OwnSlots {
    if items.len() < size {
        items.resize(size, None);
    }
    OwnSlots::new(items)
}

/// `ChestMenu`: a grid of 9 by `rows` (1-6), then the player's slots.
#[derive(Clone, Debug)]
pub struct ChestMenu {
    rows: usize,
    slots: Vec<SlotDef>,
    items: OwnSlots,
}

impl ChestMenu {
    /// A chest, barrel or ender chest has 3 rows, a double chest 6 (its first
    /// half's 27 slots first). Rows outside 1-6 are clamped.
    pub fn new(rows: usize, items: Vec<Option<ItemStack>>) -> Self {
        let rows = rows.clamp(1, 6);
        // `addChestGrid(container, 8, 18)`: index x + 9y at (8 + 18x, 18 + 18y).
        let mut slots: Vec<SlotDef> = (0..rows * 9)
            .map(|index| {
                SlotDef::own(
                    index,
                    8 + (index % 9) as i32 * 18,
                    18 + (index / 9) as i32 * 18,
                )
            })
            .collect();
        slots.extend(standard_inventory_slots(8, 18 + rows as i32 * 18 + 13));
        Self {
            rows,
            slots,
            items: container(items, rows * 9),
        }
    }

    pub fn rows(&self) -> usize {
        self.rows
    }
}

impl Menu for ChestMenu {
    fn menu_type(&self) -> &'static str {
        match self.rows {
            1 => "minecraft:generic_9x1",
            2 => "minecraft:generic_9x2",
            3 => "minecraft:generic_9x3",
            4 => "minecraft:generic_9x4",
            5 => "minecraft:generic_9x5",
            _ => "minecraft:generic_9x6",
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

    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let size = self.rows * 9;
        container_quick_move(self, cx, slot, size)
    }
}

/// `DyeColor`, the colours of the dyed shulker boxes.
pub const DYE_COLOURS: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// `Item.canFitInsideContainerItems`: false only for the items of
/// `ShulkerBoxBlock`s (`BlockItem`'s override), the plain shulker box and
/// the 16 dyed ones.
pub fn fits_inside_container_items(id: &str) -> bool {
    let name = id.strip_prefix("minecraft:").unwrap_or(id);
    let shulker_box = name == "shulker_box"
        || name
            .strip_suffix("_shulker_box")
            .is_some_and(|colour| DYE_COLOURS.contains(&colour));
    !shulker_box
}

/// `ShulkerBoxMenu`: 27 `ShulkerBoxSlot`s, which take anything that can go
/// inside a container item, so not another shulker box.
#[derive(Clone, Debug)]
pub struct ShulkerBoxMenu {
    slots: Vec<SlotDef>,
    items: OwnSlots,
}

impl ShulkerBoxMenu {
    pub fn new(items: Vec<Option<ItemStack>>) -> Self {
        let mut slots: Vec<SlotDef> = (0..27)
            .map(|index| {
                SlotDef::own(
                    index,
                    8 + (index % 9) as i32 * 18,
                    18 + (index / 9) as i32 * 18,
                )
            })
            .collect();
        slots.extend(standard_inventory_slots(8, 84));
        Self {
            slots,
            items: container(items, 27),
        }
    }
}

impl Menu for ShulkerBoxMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:shulker_box"
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

    /// `ShulkerBoxSlot.mayPlace`.
    fn may_place(&self, _cx: &MenuContext, slot: usize, stack: &ItemStack) -> bool {
        slot >= 27 || fits_inside_container_items(&stack.id)
    }

    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        container_quick_move(self, cx, slot, 27)
    }
}

/// `HopperMenu`: the hopper's 5 slots in a row.
#[derive(Clone, Debug)]
pub struct HopperMenu {
    slots: Vec<SlotDef>,
    items: OwnSlots,
}

impl HopperMenu {
    pub fn new(items: Vec<Option<ItemStack>>) -> Self {
        let mut slots: Vec<SlotDef> = (0..5)
            .map(|index| SlotDef::own(index, 44 + index as i32 * 18, 20))
            .collect();
        slots.extend(standard_inventory_slots(8, 51));
        Self {
            slots,
            items: container(items, 5),
        }
    }
}

impl Menu for HopperMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:hopper"
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

    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        container_quick_move(self, cx, slot, 5)
    }
}

/// `DispenserMenu` (`generic_3x3`): a dispenser's or dropper's 3 by 3 grid.
#[derive(Clone, Debug)]
pub struct DispenserMenu {
    slots: Vec<SlotDef>,
    items: OwnSlots,
}

impl DispenserMenu {
    pub fn new(items: Vec<Option<ItemStack>>) -> Self {
        // `add3x3GridSlots(container, 62, 17)`: index x + 3y.
        let mut slots: Vec<SlotDef> = (0..9)
            .map(|index| {
                SlotDef::own(
                    index,
                    62 + (index % 3) as i32 * 18,
                    17 + (index / 3) as i32 * 18,
                )
            })
            .collect();
        slots.extend(standard_inventory_slots(8, 84));
        Self {
            slots,
            items: container(items, 9),
        }
    }
}

impl Menu for DispenserMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:generic_3x3"
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

    /// `DispenserMenu.quickMoveStack`: as a chest's, except that it reports
    /// nothing moved when the count did not change, and calls `onTake`.
    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let mut stack = item(self, cx, slot)?.clone();
        let clicked = stack.clone();
        let moved = if slot < 9 {
            move_item_stack_to(self, cx, &mut stack, 9, 45, true)
        } else {
            move_item_stack_to(self, cx, &mut stack, 0, 9, false)
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
}
