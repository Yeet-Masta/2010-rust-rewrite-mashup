//! The engine against `AbstractContainerMenu`, `Slot` and the storage menus.
//! Every expected value is worked from the vanilla code paths, not from this
//! implementation.
use super::storage::fits_inside_container_items;
use super::*;
use crate::statistics::{CUSTOM, DROPPED};
use ContainerInput::{Clone as CloneInput, Pickup, PickupAll, QuickMove, Swap, Throw};
use serde_json::json;

/// A stack with its item's vanilla maximum.
fn st(id: &str, count: u8) -> ItemStack {
    let max = match id {
        "ender_pearl" | "glass_bottle" => 16,
        "diamond_sword" | "potion" | "shulker_box" | "white_shulker_box" | "red_shulker_box" => 1,
        _ => 64,
    };
    ItemStack {
        id: format!("minecraft:{id}"),
        count,
        max,
        components: None,
    }
}

fn renamed(id: &str, count: u8) -> ItemStack {
    ItemStack {
        components: Some(json!({"minecraft:custom_name": "Kept"})),
        ..st(id, count)
    }
}

fn grid(size: usize, stacks: &[(usize, ItemStack)]) -> Vec<Option<ItemStack>> {
    let mut items = vec![None; size];
    for (index, stack) in stacks {
        items[*index] = Some(stack.clone());
    }
    items
}

/// The menu slot showing inventory slot `inv`, after `container` own slots:
/// main 9-35 first, then the hotbar.
fn player_slot(container: usize, inv: usize) -> usize {
    if inv < 9 {
        container + 27 + inv
    } else {
        container + inv - 9
    }
}

struct Rig {
    inventory: Inventory,
    random: LegacyRandom,
}

fn rig() -> Rig {
    Rig {
        inventory: Inventory::default(),
        random: LegacyRandom::new(0),
    }
}

impl Rig {
    fn cx(&mut self) -> MenuContext<'_> {
        MenuContext::new(&mut self.inventory, &mut self.random)
    }
}

fn press<M: Menu>(
    menu: &mut M,
    cx: &mut MenuContext,
    slot: usize,
    button: i32,
    kind: ContainerInput,
) {
    assert!(handle(
        menu,
        cx,
        &MenuInput::Click {
            slot: slot as i32,
            button,
            kind
        }
    ));
}

fn outside<M: Menu>(menu: &mut M, cx: &mut MenuContext, button: i32, kind: ContainerInput) {
    assert!(handle(
        menu,
        cx,
        &MenuInput::Click {
            slot: SLOT_CLICKED_OUTSIDE,
            button,
            kind
        }
    ));
}

fn drag_over<M: Menu>(menu: &mut M, cx: &mut MenuContext, button: i32, slots: &[usize]) {
    assert!(handle(
        menu,
        cx,
        &MenuInput::Drag {
            button,
            slots: slots.to_vec()
        }
    ));
}

fn own<M: Menu>(menu: &M, index: usize) -> Option<ItemStack> {
    menu.own().get(index).cloned()
}

fn chest(stacks: &[(usize, ItemStack)]) -> ChestMenu {
    ChestMenu::new(3, grid(27, stacks))
}

/// The player's inventory slot `index`.
fn inv(cx: &MenuContext, index: usize) -> Option<ItemStack> {
    cx.inventory.slots[index].clone()
}

fn fill_inventory(inventory: &mut Inventory, stack: &ItemStack) {
    for slot in &mut inventory.slots[..36] {
        *slot = Some(stack.clone());
    }
}

/// Own slots 0-2 hold one item each (as `BrewingStandMenu.PotionSlot`
/// does), 3 is plain, 4 is an output that takes nothing (as
/// `FurnaceResultSlot`) and refills from a reserve when emptied (as a
/// crafting result does), 5 can't be picked up. Then the player's slots.
struct TestMenu {
    slots: Vec<SlotDef>,
    items: OwnSlots,
    refill: ItemStack,
    reserve: u8,
    taken: Vec<(usize, u8)>,
    swap_crafts: Vec<(usize, i32)>,
    changed: Vec<usize>,
}

const OUTPUT: usize = 4;
const LOCKED: usize = 5;
/// The first player slot (inventory 9) of a `TestMenu`.
const PLAYER: usize = 6;

impl TestMenu {
    fn new(stacks: &[(usize, ItemStack)]) -> Self {
        let mut slots: Vec<SlotDef> = (0..PLAYER).map(|index| SlotDef::own(index, 0, 0)).collect();
        slots.extend(standard_inventory_slots(8, 84));
        Self {
            slots,
            items: OwnSlots::new(grid(PLAYER, stacks)),
            refill: st("iron_ingot", 4),
            reserve: 0,
            taken: Vec::new(),
            swap_crafts: Vec::new(),
            changed: Vec::new(),
        }
    }
}

impl Menu for TestMenu {
    fn menu_type(&self) -> &'static str {
        "test"
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
        slot != OUTPUT
    }

    fn may_pickup(&self, _cx: &MenuContext, slot: usize) -> bool {
        slot != LOCKED
    }

    fn max_stack(&self, _cx: &MenuContext, slot: usize, stack: &ItemStack) -> i32 {
        if slot < 3 {
            1
        } else {
            CONTAINER_MAX_STACK.min(i32::from(stack.max))
        }
    }

    /// Own slots into the player's, last first (with `onTake` for the
    /// output, as `CraftingMenu` does); the player's into own slots 0-3.
    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let mut stack = item(self, cx, slot)?.clone();
        let clicked = stack.clone();
        let len = self.slots.len();
        let moved = if slot < PLAYER {
            move_item_stack_to(self, cx, &mut stack, PLAYER, len, true)
        } else {
            move_item_stack_to(self, cx, &mut stack, 0, 4, false)
        };
        if !moved {
            return None;
        }
        let left = stack.clone();
        put_back(self, cx, slot, stack);
        if slot == OUTPUT {
            self.on_take(cx, slot, &left);
        }
        Some(clicked)
    }

    fn on_take(&mut self, cx: &mut MenuContext, slot: usize, taken: &ItemStack) {
        self.taken.push((slot, taken.count));
        if slot == OUTPUT && self.items.get(OUTPUT).is_none() && self.reserve > 0 {
            self.reserve -= 1;
            let refill = self.refill.clone();
            set(self, cx, OUTPUT, Some(refill));
        }
        set_changed(self, cx, slot);
    }

    fn on_swap_craft(&mut self, _cx: &mut MenuContext, slot: usize, count: i32) {
        self.swap_crafts.push((slot, count));
    }

    fn slots_changed(&mut self, _cx: &mut MenuContext, slot: usize) {
        self.changed.push(slot);
    }

    fn can_take_for_pick_all(&self, _carried: &ItemStack, slot: usize) -> bool {
        slot != OUTPUT
    }
}

// Layouts (the menus' constructors).

#[test]
fn chest_menus_lay_out_every_row_count_as_vanilla() {
    for rows in 1..=6usize {
        let menu = ChestMenu::new(rows, Vec::new());
        let size = rows * 9;
        let slots = menu.slots();
        assert_eq!(slots.len(), size + 36);
        assert_eq!(menu.own().len(), size);
        assert_eq!(menu.menu_type(), format!("minecraft:generic_9x{rows}"));
        assert_eq!(slots[0], SlotDef::own(0, 8, 18));
        assert_eq!(slots[8], SlotDef::own(8, 152, 18));
        if rows > 1 {
            assert_eq!(slots[10], SlotDef::own(10, 26, 36));
        }
        assert_eq!(
            slots[size - 1],
            SlotDef::own(size - 1, 152, 18 + 18 * (rows as i32 - 1))
        );
        // `18 + rows * 18 + 13`, the hotbar 58 lower.
        let top = 31 + 18 * rows as i32;
        assert_eq!(slots[size], SlotDef::player(9, 8, top));
        assert_eq!(slots[size + 26], SlotDef::player(35, 152, top + 36));
        assert_eq!(slots[size + 27], SlotDef::player(0, 8, top + 58));
        assert_eq!(slots[size + 35], SlotDef::player(8, 152, top + 58));
    }
    // 04-vanilla-menus' table: 3 rows put the inventory at 85 and the hotbar
    // at 143, 6 rows at 139 and 197.
    assert_eq!(ChestMenu::new(3, Vec::new()).slots()[27].y, 85);
    assert_eq!(ChestMenu::new(6, Vec::new()).slots()[81].y, 197);
    assert_eq!(ChestMenu::new(0, Vec::new()).rows(), 1);
    assert_eq!(ChestMenu::new(9, Vec::new()).rows(), 6);
}

#[test]
fn chest_storage_is_padded_and_never_truncated() {
    let short = ChestMenu::new(3, grid(10, &[(9, st("stone", 1))]));
    assert_eq!(short.own().len(), 27);
    assert_eq!(own(&short, 9), Some(st("stone", 1)));
    let long = ChestMenu::new(1, grid(27, &[(20, st("stone", 1))]));
    assert_eq!(long.own().len(), 27);
    assert_eq!(long.slots().len(), 45);
    assert_eq!(own(&long, 20), Some(st("stone", 1)));
}

#[test]
fn shulker_hopper_and_dispenser_lay_out_as_vanilla() {
    let shulker = ShulkerBoxMenu::new(Vec::new());
    let slots = shulker.slots();
    assert_eq!(shulker.menu_type(), "minecraft:shulker_box");
    assert_eq!(slots.len(), 63);
    assert_eq!(slots[0], SlotDef::own(0, 8, 18));
    assert_eq!(slots[26], SlotDef::own(26, 152, 54));
    assert_eq!(slots[27], SlotDef::player(9, 8, 84));
    assert_eq!(slots[53], SlotDef::player(35, 152, 120));
    assert_eq!(slots[54], SlotDef::player(0, 8, 142));
    assert_eq!(slots[62], SlotDef::player(8, 152, 142));

    let hopper = HopperMenu::new(Vec::new());
    let slots = hopper.slots();
    assert_eq!(hopper.menu_type(), "minecraft:hopper");
    assert_eq!(slots.len(), 41);
    assert_eq!(hopper.own().len(), 5);
    assert_eq!(slots[0], SlotDef::own(0, 44, 20));
    assert_eq!(slots[4], SlotDef::own(4, 116, 20));
    assert_eq!(slots[5], SlotDef::player(9, 8, 51));
    assert_eq!(slots[31], SlotDef::player(35, 152, 87));
    assert_eq!(slots[32], SlotDef::player(0, 8, 109));
    assert_eq!(slots[40], SlotDef::player(8, 152, 109));

    let dispenser = DispenserMenu::new(Vec::new());
    let slots = dispenser.slots();
    assert_eq!(dispenser.menu_type(), "minecraft:generic_3x3");
    assert_eq!(slots.len(), 45);
    assert_eq!(dispenser.own().len(), 9);
    assert_eq!(slots[0], SlotDef::own(0, 62, 17));
    assert_eq!(slots[2], SlotDef::own(2, 98, 17));
    assert_eq!(slots[3], SlotDef::own(3, 62, 35));
    assert_eq!(slots[8], SlotDef::own(8, 98, 53));
    assert_eq!(slots[9], SlotDef::player(9, 8, 84));
    assert_eq!(slots[36], SlotDef::player(0, 8, 142));
    assert_eq!(slots[44], SlotDef::player(8, 152, 142));
}

// PICKUP.

#[test]
fn left_pickup_takes_the_stack_and_right_the_larger_half() {
    let mut rig = rig();
    let mut menu = chest(&[
        (0, st("stone", 7)),
        (1, st("stone", 7)),
        (2, st("stone", 8)),
        (3, st("stone", 1)),
    ]);
    let mut cx = rig.cx();
    press(&mut menu, &mut cx, 0, 0, Pickup);
    assert_eq!(cx.inventory.cursor, Some(st("stone", 7)));
    assert_eq!(own(&menu, 0), None);

    cx.inventory.cursor = None;
    press(&mut menu, &mut cx, 1, 1, Pickup);
    // `(count + 1) / 2`.
    assert_eq!(cx.inventory.cursor, Some(st("stone", 4)));
    assert_eq!(own(&menu, 1), Some(st("stone", 3)));

    cx.inventory.cursor = None;
    press(&mut menu, &mut cx, 2, 1, Pickup);
    assert_eq!(cx.inventory.cursor, Some(st("stone", 4)));
    assert_eq!(own(&menu, 2), Some(st("stone", 4)));

    cx.inventory.cursor = None;
    press(&mut menu, &mut cx, 3, 1, Pickup);
    assert_eq!(cx.inventory.cursor, Some(st("stone", 1)));
    assert_eq!(own(&menu, 3), None);
}

#[test]
fn left_place_puts_everything_and_right_puts_one() {
    let mut rig = rig();
    let mut menu = chest(&[]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("stone", 10));
    press(&mut menu, &mut cx, 3, 0, Pickup);
    assert_eq!(own(&menu, 3), Some(st("stone", 10)));
    assert_eq!(cx.inventory.cursor, None);

    cx.inventory.cursor = Some(st("stone", 10));
    press(&mut menu, &mut cx, 4, 1, Pickup);
    assert_eq!(own(&menu, 4), Some(st("stone", 1)));
    assert_eq!(cx.inventory.cursor, Some(st("stone", 9)));
    // Onto the same item, a right click adds one.
    press(&mut menu, &mut cx, 4, 1, Pickup);
    assert_eq!(own(&menu, 4), Some(st("stone", 2)));
    assert_eq!(cx.inventory.cursor, Some(st("stone", 8)));
}

#[test]
fn placing_onto_the_same_item_merges_up_to_its_maximum() {
    let mut rig = rig();
    let mut menu = chest(&[
        (0, st("stone", 20)),
        (1, st("stone", 64)),
        (2, st("ender_pearl", 10)),
    ]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("stone", 50));
    press(&mut menu, &mut cx, 0, 0, Pickup);
    assert_eq!(own(&menu, 0), Some(st("stone", 64)));
    assert_eq!(cx.inventory.cursor, Some(st("stone", 6)));

    // A full stack takes nothing, and nothing is swapped.
    cx.inventory.cursor = Some(st("stone", 10));
    press(&mut menu, &mut cx, 1, 0, Pickup);
    press(&mut menu, &mut cx, 1, 1, Pickup);
    assert_eq!(own(&menu, 1), Some(st("stone", 64)));
    assert_eq!(cx.inventory.cursor, Some(st("stone", 10)));

    // Ender pearls stop at 16.
    cx.inventory.cursor = Some(st("ender_pearl", 10));
    press(&mut menu, &mut cx, 2, 0, Pickup);
    assert_eq!(own(&menu, 2), Some(st("ender_pearl", 16)));
    assert_eq!(cx.inventory.cursor, Some(st("ender_pearl", 4)));
}

#[test]
fn a_different_item_swaps_with_either_button() {
    for button in [0, 1] {
        let mut rig = rig();
        let mut menu = chest(&[(5, st("dirt", 5))]);
        let mut cx = rig.cx();
        cx.inventory.cursor = Some(st("stone", 10));
        press(&mut menu, &mut cx, 5, button, Pickup);
        assert_eq!(own(&menu, 5), Some(st("stone", 10)));
        assert_eq!(cx.inventory.cursor, Some(st("dirt", 5)));
    }
    // Other components make another item.
    let mut rig = rig();
    let mut menu = chest(&[(5, renamed("stone", 5))]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("stone", 10));
    press(&mut menu, &mut cx, 5, 0, Pickup);
    assert_eq!(own(&menu, 5), Some(st("stone", 10)));
    assert_eq!(cx.inventory.cursor, Some(renamed("stone", 5)));
}

#[test]
fn a_slot_that_holds_one_takes_one() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("glass_bottle", 5));
    press(&mut menu, &mut cx, 0, 0, Pickup);
    assert_eq!(own(&menu, 0), Some(st("glass_bottle", 1)));
    assert_eq!(cx.inventory.cursor, Some(st("glass_bottle", 4)));
    // Full at one: nothing more goes in.
    press(&mut menu, &mut cx, 0, 0, Pickup);
    press(&mut menu, &mut cx, 0, 1, Pickup);
    assert_eq!(own(&menu, 0), Some(st("glass_bottle", 1)));
    assert_eq!(cx.inventory.cursor, Some(st("glass_bottle", 4)));
    press(&mut menu, &mut cx, 1, 1, Pickup);
    assert_eq!(own(&menu, 1), Some(st("glass_bottle", 1)));
    assert_eq!(cx.inventory.cursor, Some(st("glass_bottle", 3)));
}

#[test]
fn a_swap_needs_the_carried_stack_to_fit_the_slot() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[(0, st("potion", 1))]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("glass_bottle", 5));
    press(&mut menu, &mut cx, 0, 0, Pickup);
    press(&mut menu, &mut cx, 0, 1, Pickup);
    assert_eq!(own(&menu, 0), Some(st("potion", 1)));
    assert_eq!(cx.inventory.cursor, Some(st("glass_bottle", 5)));

    cx.inventory.cursor = Some(st("glass_bottle", 1));
    press(&mut menu, &mut cx, 0, 0, Pickup);
    assert_eq!(own(&menu, 0), Some(st("glass_bottle", 1)));
    assert_eq!(cx.inventory.cursor, Some(st("potion", 1)));
}

#[test]
fn an_output_gives_its_whole_stack_onto_a_matching_cursor_or_nothing() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[(OUTPUT, st("iron_ingot", 10))]);
    let mut cx = rig.cx();
    // 4 more fit on the cursor, so none of the 10 move.
    cx.inventory.cursor = Some(st("iron_ingot", 60));
    press(&mut menu, &mut cx, OUTPUT, 0, Pickup);
    assert_eq!(own(&menu, OUTPUT), Some(st("iron_ingot", 10)));
    assert_eq!(cx.inventory.cursor, Some(st("iron_ingot", 60)));
    assert!(menu.taken.is_empty());

    cx.inventory.cursor = Some(st("iron_ingot", 50));
    press(&mut menu, &mut cx, OUTPUT, 0, Pickup);
    assert_eq!(own(&menu, OUTPUT), None);
    assert_eq!(cx.inventory.cursor, Some(st("iron_ingot", 60)));
    assert_eq!(menu.taken, [(OUTPUT, 10)]);

    // Another item neither swaps nor goes in.
    set(&mut menu, &mut cx, OUTPUT, Some(st("iron_ingot", 10)));
    cx.inventory.cursor = Some(st("dirt", 5));
    press(&mut menu, &mut cx, OUTPUT, 0, Pickup);
    assert_eq!(own(&menu, OUTPUT), Some(st("iron_ingot", 10)));
    assert_eq!(cx.inventory.cursor, Some(st("dirt", 5)));
    // And nothing goes into an empty output.
    set(&mut menu, &mut cx, OUTPUT, None);
    press(&mut menu, &mut cx, OUTPUT, 0, Pickup);
    assert_eq!(own(&menu, OUTPUT), None);
    assert_eq!(cx.inventory.cursor, Some(st("dirt", 5)));
}

#[test]
fn an_empty_cursor_takes_all_or_half_of_an_output() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[(OUTPUT, st("iron_ingot", 10))]);
    let mut cx = rig.cx();
    // `tryRemove(5, MAX)`: a furnace output splits like any container.
    press(&mut menu, &mut cx, OUTPUT, 1, Pickup);
    assert_eq!(cx.inventory.cursor, Some(st("iron_ingot", 5)));
    assert_eq!(own(&menu, OUTPUT), Some(st("iron_ingot", 5)));
    cx.inventory.cursor = None;
    press(&mut menu, &mut cx, OUTPUT, 0, Pickup);
    assert_eq!(cx.inventory.cursor, Some(st("iron_ingot", 5)));
    assert_eq!(own(&menu, OUTPUT), None);
    assert_eq!(menu.taken, [(OUTPUT, 5), (OUTPUT, 5)]);
}

#[test]
fn a_slot_that_cannot_be_picked_up_still_takes_items() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[(LOCKED, st("stone", 3))]);
    let mut cx = rig.cx();
    cx.inventory.slots[1] = Some(st("dirt", 2));
    press(&mut menu, &mut cx, LOCKED, 0, Pickup);
    press(&mut menu, &mut cx, LOCKED, 0, QuickMove);
    press(&mut menu, &mut cx, LOCKED, 1, Throw);
    press(&mut menu, &mut cx, LOCKED, 0, Swap);
    press(&mut menu, &mut cx, LOCKED, 1, Swap);
    assert_eq!(own(&menu, LOCKED), Some(st("stone", 3)));
    assert_eq!(cx.inventory.cursor, None);
    assert_eq!(inv(&cx, 0), None);
    assert_eq!(inv(&cx, 1), Some(st("dirt", 2)));
    assert!(cx.thrown.is_empty());

    // Empty, it takes a click's stack and a hotbar key's.
    set(&mut menu, &mut cx, LOCKED, None);
    cx.inventory.cursor = Some(st("stone", 4));
    press(&mut menu, &mut cx, LOCKED, 0, Pickup);
    assert_eq!(own(&menu, LOCKED), Some(st("stone", 4)));
    assert_eq!(cx.inventory.cursor, None);
    set(&mut menu, &mut cx, LOCKED, None);
    press(&mut menu, &mut cx, LOCKED, 1, Swap);
    assert_eq!(own(&menu, LOCKED), Some(st("dirt", 2)));
    assert_eq!(inv(&cx, 1), None);
}

#[test]
fn clicks_outside_the_window_drop_the_carried_stack() {
    let mut rig = rig();
    let mut menu = chest(&[]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("stone", 10));
    outside(&mut menu, &mut cx, 1, Pickup);
    assert_eq!(cx.thrown, [st("stone", 1)]);
    assert_eq!(cx.inventory.cursor, Some(st("stone", 9)));
    outside(&mut menu, &mut cx, 0, Pickup);
    assert_eq!(cx.thrown, [st("stone", 1), st("stone", 9)]);
    assert_eq!(cx.inventory.cursor, None);
    // A shift-click outside drops too.
    cx.inventory.cursor = Some(st("dirt", 3));
    outside(&mut menu, &mut cx, 0, QuickMove);
    assert_eq!(cx.thrown.last(), Some(&st("dirt", 3)));
    assert_eq!(cx.inventory.cursor, None);
    // Thrown from the hand: counted.
    assert_eq!(
        cx.inventory.take_stat_events(),
        [
            (DROPPED.to_owned(), "minecraft:stone".to_owned(), 1),
            (CUSTOM.to_owned(), "minecraft:drop".to_owned(), 1),
            (DROPPED.to_owned(), "minecraft:stone".to_owned(), 9),
            (CUSTOM.to_owned(), "minecraft:drop".to_owned(), 1),
            (DROPPED.to_owned(), "minecraft:dirt".to_owned(), 3),
            (CUSTOM.to_owned(), "minecraft:drop".to_owned(), 1),
        ]
    );

    // Nothing carried, another button, slot -1, a slot past the end, or a
    // THROW outside: nothing.
    outside(&mut menu, &mut cx, 0, Pickup);
    cx.inventory.cursor = Some(st("stone", 10));
    outside(&mut menu, &mut cx, 2, Pickup);
    assert!(handle(
        &mut menu,
        &mut cx,
        &MenuInput::Click {
            slot: -1,
            button: 0,
            kind: Pickup
        }
    ));
    press(&mut menu, &mut cx, 63, 0, Pickup);
    cx.inventory.cursor = None;
    outside(&mut menu, &mut cx, 0, Throw);
    assert_eq!(cx.thrown.len(), 3);
}

// QUICK_MOVE.

#[test]
fn shift_clicking_out_of_a_chest_fills_the_hotbar_from_the_right() {
    let mut rig = rig();
    let mut menu = chest(&[(0, st("stone", 10)), (1, st("cobblestone", 10))]);
    let mut cx = rig.cx();
    press(&mut menu, &mut cx, 0, 0, QuickMove);
    assert_eq!(own(&menu, 0), None);
    assert_eq!(inv(&cx, 8), Some(st("stone", 10)));
    assert_eq!(menu.slots()[62].at, SlotRef::Player(8));

    // With the hotbar full of something else, the main inventory from its
    // last slot.
    for slot in &mut cx.inventory.slots[..9] {
        *slot = Some(st("dirt", 64));
    }
    press(&mut menu, &mut cx, 1, 1, QuickMove);
    assert_eq!(own(&menu, 1), None);
    assert_eq!(inv(&cx, 35), Some(st("cobblestone", 10)));
}

#[test]
fn shift_clicking_out_tops_up_matching_stacks_before_an_empty_slot() {
    let mut rig = rig();
    let mut menu = chest(&[(0, st("stone", 30))]);
    let mut cx = rig.cx();
    cx.inventory.slots[2] = Some(st("stone", 60));
    cx.inventory.slots[20] = Some(st("stone", 50));
    press(&mut menu, &mut cx, 0, 0, QuickMove);
    // Backwards over main-then-hotbar: hotbar 2 before main 20, and what is
    // left into the last empty slot, hotbar 8.
    assert_eq!(inv(&cx, 2), Some(st("stone", 64)));
    assert_eq!(inv(&cx, 20), Some(st("stone", 64)));
    assert_eq!(inv(&cx, 8), Some(st("stone", 12)));
    assert_eq!(own(&menu, 0), None);
    assert_eq!(cx.player_slots_written().collect::<Vec<_>>(), [2, 8, 20]);
}

#[test]
fn shift_clicking_into_a_chest_fills_it_from_the_first_slot() {
    let mut rig = rig();
    let mut menu = chest(&[(2, st("stone", 60)), (5, st("stone", 60))]);
    let mut cx = rig.cx();
    cx.inventory.slots[9] = Some(st("stone", 10));
    press(&mut menu, &mut cx, player_slot(27, 9), 0, QuickMove);
    assert_eq!(own(&menu, 2), Some(st("stone", 64)));
    assert_eq!(own(&menu, 5), Some(st("stone", 64)));
    assert_eq!(own(&menu, 0), Some(st("stone", 2)));
    assert_eq!(inv(&cx, 9), None);
    assert_eq!(menu.own_mut().take_changed(), [0, 2, 5]);
}

#[test]
fn shift_clicking_with_no_room_moves_nothing() {
    let mut rig = rig();
    let full: Vec<(usize, ItemStack)> = (0..27).map(|index| (index, st("dirt", 64))).collect();
    let mut menu = chest(&full);
    let mut cx = rig.cx();
    cx.inventory.slots[9] = Some(st("stone", 10));
    press(&mut menu, &mut cx, player_slot(27, 9), 0, QuickMove);
    assert_eq!(inv(&cx, 9), Some(st("stone", 10)));
    assert!(menu.own_mut().take_changed().is_empty());
    // An empty slot moves nothing either.
    press(&mut menu, &mut cx, player_slot(27, 10), 0, QuickMove);
    assert_eq!(inv(&cx, 10), None);
}

#[test]
fn a_double_chest_shift_clicks_across_its_six_rows() {
    let mut rig = rig();
    let mut menu = ChestMenu::new(6, grid(54, &[(53, st("stone", 5))]));
    let mut cx = rig.cx();
    cx.inventory.slots[0] = Some(st("dirt", 5));
    press(&mut menu, &mut cx, 53, 0, QuickMove);
    assert_eq!(inv(&cx, 8), Some(st("stone", 5)));
    press(&mut menu, &mut cx, player_slot(54, 0), 0, QuickMove);
    assert_eq!(own(&menu, 0), Some(st("dirt", 5)));
    assert_eq!(inv(&cx, 0), None);
}

#[test]
fn quick_move_fills_one_empty_slot_per_pass_and_repeats() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[]);
    let mut cx = rig.cx();
    cx.inventory.slots[0] = Some(st("glass_bottle", 5));
    press(&mut menu, &mut cx, player_slot(PLAYER, 0), 0, QuickMove);
    // One bottle into each one-item slot, a pass each, then the last two
    // together into the plain slot.
    assert_eq!(own(&menu, 0), Some(st("glass_bottle", 1)));
    assert_eq!(own(&menu, 1), Some(st("glass_bottle", 1)));
    assert_eq!(own(&menu, 2), Some(st("glass_bottle", 1)));
    assert_eq!(own(&menu, 3), Some(st("glass_bottle", 2)));
    assert_eq!(inv(&cx, 0), None);

    // The merge pass first tops up the plain slot; once the slots are full
    // the loop stops with one bottle left.
    let mut rig = self::rig();
    let mut menu = TestMenu::new(&[(3, st("glass_bottle", 15))]);
    let mut cx = rig.cx();
    cx.inventory.slots[0] = Some(st("glass_bottle", 5));
    press(&mut menu, &mut cx, player_slot(PLAYER, 0), 0, QuickMove);
    assert_eq!(own(&menu, 3), Some(st("glass_bottle", 16)));
    for slot in 0..3 {
        assert_eq!(own(&menu, slot), Some(st("glass_bottle", 1)));
    }
    assert_eq!(inv(&cx, 0), Some(st("glass_bottle", 1)));
}

#[test]
fn move_item_stack_to_fills_a_single_empty_slot_per_call() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[]);
    let mut cx = rig.cx();
    let mut stack = st("glass_bottle", 5);
    assert!(move_item_stack_to(
        &mut menu, &mut cx, &mut stack, 0, 4, false
    ));
    assert_eq!(stack.count, 4);
    assert_eq!(own(&menu, 0), Some(st("glass_bottle", 1)));
    assert_eq!(own(&menu, 1), None);
    // Backwards: the last empty slot of the range.
    assert!(move_item_stack_to(
        &mut menu, &mut cx, &mut stack, 0, 4, true
    ));
    assert_eq!(stack.count, 0);
    assert_eq!(own(&menu, 3), Some(st("glass_bottle", 4)));
    // Nothing to move: nothing moved.
    assert!(!move_item_stack_to(
        &mut menu, &mut cx, &mut stack, 0, 4, false
    ));
}

#[test]
fn move_item_stack_to_merges_without_asking_may_place() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[(OUTPUT, st("iron_ingot", 10))]);
    let mut cx = rig.cx();
    let mut stack = st("iron_ingot", 5);
    assert!(move_item_stack_to(
        &mut menu, &mut cx, &mut stack, 3, 6, false
    ));
    assert_eq!(stack.count, 0);
    assert_eq!(own(&menu, OUTPUT), Some(st("iron_ingot", 15)));
    assert_eq!(own(&menu, 3), None);
    // But an empty output takes nothing.
    let mut menu = TestMenu::new(&[(3, st("dirt", 1))]);
    let mut stack = st("iron_ingot", 5);
    assert!(!move_item_stack_to(
        &mut menu, &mut cx, &mut stack, 3, 5, false
    ));
    assert_eq!(stack.count, 5);
    // A range past the slots is cut short: 40 is hotbar 7, the last 8.
    assert!(move_item_stack_to(
        &mut menu, &mut cx, &mut stack, 40, 99, false
    ));
    assert_eq!(inv(&cx, 7), Some(st("iron_ingot", 5)));
    assert_eq!(inv(&cx, 8), None);
}

#[test]
fn quick_move_repeats_while_an_output_refills() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[(OUTPUT, st("iron_ingot", 4))]);
    menu.reserve = 2;
    let mut cx = rig.cx();
    press(&mut menu, &mut cx, OUTPUT, 0, QuickMove);
    assert_eq!(inv(&cx, 8), Some(st("iron_ingot", 12)));
    assert_eq!(own(&menu, OUTPUT), None);
    assert_eq!(menu.reserve, 0);
    assert_eq!(menu.taken, [(OUTPUT, 0), (OUTPUT, 0), (OUTPUT, 0)]);
}

// SWAP.

#[test]
fn hotbar_keys_swap_with_container_slots() {
    let mut rig = rig();
    let mut menu = chest(&[(4, st("stone", 10))]);
    let mut cx = rig.cx();
    press(&mut menu, &mut cx, 4, 3, Swap);
    assert_eq!(inv(&cx, 3), Some(st("stone", 10)));
    assert_eq!(own(&menu, 4), None);
    press(&mut menu, &mut cx, 4, 3, Swap);
    assert_eq!(own(&menu, 4), Some(st("stone", 10)));
    assert_eq!(inv(&cx, 3), None);

    // A full stack for a full stack.
    set(&mut menu, &mut cx, 4, Some(st("dirt", 64)));
    cx.inventory.slots[3] = Some(st("stone", 64));
    press(&mut menu, &mut cx, 4, 3, Swap);
    assert_eq!(own(&menu, 4), Some(st("stone", 64)));
    assert_eq!(inv(&cx, 3), Some(st("dirt", 64)));

    // The offhand key.
    set(&mut menu, &mut cx, 6, Some(st("ender_pearl", 5)));
    press(&mut menu, &mut cx, 6, OFFHAND, Swap);
    assert_eq!(inv(&cx, 40), Some(st("ender_pearl", 5)));
    assert_eq!(own(&menu, 6), None);

    // Within the player's slots.
    cx.inventory.slots[9] = Some(st("stone", 7));
    cx.inventory.slots[0] = Some(st("dirt", 2));
    press(&mut menu, &mut cx, player_slot(27, 9), 0, Swap);
    assert_eq!(inv(&cx, 9), Some(st("dirt", 2)));
    assert_eq!(inv(&cx, 0), Some(st("stone", 7)));
    // A hotbar slot with its own key: nothing changes.
    press(&mut menu, &mut cx, player_slot(27, 0), 0, Swap);
    assert_eq!(inv(&cx, 0), Some(st("stone", 7)));
}

#[test]
fn swaps_with_other_buttons_or_no_slot_do_nothing() {
    let mut rig = rig();
    let mut menu = chest(&[(4, st("stone", 10))]);
    let mut cx = rig.cx();
    for button in [9, 35, 39, 41, -1] {
        press(&mut menu, &mut cx, 4, button, Swap);
    }
    for slot in [-1, SLOT_CLICKED_OUTSIDE, 63] {
        assert!(handle(
            &mut menu,
            &mut cx,
            &MenuInput::Click {
                slot,
                button: 0,
                kind: Swap
            }
        ));
    }
    assert_eq!(own(&menu, 4), Some(st("stone", 10)));
    assert!(cx.inventory.slots.iter().all(Option::is_none));
}

#[test]
fn a_swap_into_a_one_item_slot_splits_the_stack() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[]);
    let mut cx = rig.cx();
    cx.inventory.slots[0] = Some(st("glass_bottle", 16));
    press(&mut menu, &mut cx, 0, 0, Swap);
    assert_eq!(own(&menu, 0), Some(st("glass_bottle", 1)));
    assert_eq!(inv(&cx, 0), Some(st("glass_bottle", 15)));

    // Taking back out: `onSwapCraft` and `onTake`.
    press(&mut menu, &mut cx, 0, 1, Swap);
    assert_eq!(inv(&cx, 1), Some(st("glass_bottle", 1)));
    assert_eq!(own(&menu, 0), None);
    assert_eq!(menu.swap_crafts, [(0, 1)]);
    assert_eq!(menu.taken, [(0, 1)]);
}

#[test]
fn a_full_stack_into_a_full_one_item_slot_moves_the_old_item_into_the_inventory() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[(0, st("potion", 1))]);
    let mut cx = rig.cx();
    cx.inventory.slots[0] = Some(st("glass_bottle", 16));
    press(&mut menu, &mut cx, 0, 0, Swap);
    assert_eq!(own(&menu, 0), Some(st("glass_bottle", 1)));
    assert_eq!(inv(&cx, 0), Some(st("glass_bottle", 15)));
    // `Inventory.add`: no stack with room, so the first free slot.
    assert_eq!(inv(&cx, 1), Some(st("potion", 1)));
    assert_eq!(menu.taken, [(0, 1)]);
    assert!(cx.thrown.is_empty());
}

#[test]
fn the_old_item_is_dropped_when_the_inventory_is_full_and_lost_in_creative() {
    for creative in [false, true] {
        let mut rig = rig();
        fill_inventory(&mut rig.inventory, &st("dirt", 64));
        rig.inventory.slots[0] = Some(st("glass_bottle", 16));
        let mut menu = TestMenu::new(&[(0, st("potion", 1))]);
        let mut cx = rig.cx();
        cx.creative = creative;
        press(&mut menu, &mut cx, 0, 0, Swap);
        assert_eq!(own(&menu, 0), Some(st("glass_bottle", 1)));
        assert_eq!(inv(&cx, 0), Some(st("glass_bottle", 15)));
        if creative {
            assert!(cx.thrown.is_empty());
        } else {
            assert_eq!(cx.thrown, [st("potion", 1)]);
            assert_eq!(
                cx.inventory.take_stat_events()[0],
                (DROPPED.to_owned(), "minecraft:potion".to_owned(), 1)
            );
        }
    }
}

// CLONE.

#[test]
fn a_creative_middle_click_copies_a_full_stack() {
    let mut rig = rig();
    let mut menu = chest(&[(0, st("ender_pearl", 3))]);
    let mut cx = rig.cx();
    press(&mut menu, &mut cx, 0, 2, CloneInput);
    assert_eq!(cx.inventory.cursor, None);

    cx.creative = true;
    press(&mut menu, &mut cx, 1, 2, CloneInput);
    assert_eq!(cx.inventory.cursor, None);
    press(&mut menu, &mut cx, 0, 2, CloneInput);
    assert_eq!(cx.inventory.cursor, Some(st("ender_pearl", 16)));
    assert_eq!(own(&menu, 0), Some(st("ender_pearl", 3)));
    // Only with nothing carried.
    cx.inventory.cursor = Some(st("dirt", 1));
    press(&mut menu, &mut cx, 0, 2, CloneInput);
    assert_eq!(cx.inventory.cursor, Some(st("dirt", 1)));
}

// THROW.

#[test]
fn the_drop_key_throws_one_or_the_stack() {
    let mut rig = rig();
    let mut menu = chest(&[
        (0, st("stone", 10)),
        (1, st("dirt", 1)),
        (2, st("cobblestone", 5)),
    ]);
    let mut cx = rig.cx();
    press(&mut menu, &mut cx, 0, 0, Throw);
    assert_eq!(cx.thrown, [st("stone", 1)]);
    assert_eq!(own(&menu, 0), Some(st("stone", 9)));
    press(&mut menu, &mut cx, 0, 1, Throw);
    assert_eq!(cx.thrown[1], st("stone", 9));
    assert_eq!(own(&menu, 0), None);
    press(&mut menu, &mut cx, 1, 0, Throw);
    assert_eq!(own(&menu, 1), None);
    // Any other button throws the stack, once.
    press(&mut menu, &mut cx, 2, 5, Throw);
    assert_eq!(
        cx.thrown,
        [
            st("stone", 1),
            st("stone", 9),
            st("dirt", 1),
            st("cobblestone", 5)
        ]
    );
    assert_eq!(cx.inventory.take_stat_events().len(), 8);

    // Not while carrying, and nothing from an empty slot.
    set(&mut menu, &mut cx, 3, Some(st("stone", 4)));
    cx.inventory.cursor = Some(st("dirt", 1));
    press(&mut menu, &mut cx, 3, 1, Throw);
    cx.inventory.cursor = None;
    press(&mut menu, &mut cx, 4, 1, Throw);
    assert_eq!(own(&menu, 3), Some(st("stone", 4)));
    assert_eq!(cx.thrown.len(), 4);
}

#[test]
fn the_stack_drop_repeats_while_an_output_refills() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[(OUTPUT, st("iron_ingot", 4))]);
    menu.reserve = 2;
    let mut cx = rig.cx();
    press(&mut menu, &mut cx, OUTPUT, 1, Throw);
    assert_eq!(
        cx.thrown,
        [
            st("iron_ingot", 4),
            st("iron_ingot", 4),
            st("iron_ingot", 4)
        ]
    );
    assert_eq!(own(&menu, OUTPUT), None);

    // One at a time from an output.
    let mut rig = self::rig();
    let mut menu = TestMenu::new(&[(OUTPUT, st("iron_ingot", 4))]);
    let mut cx = rig.cx();
    press(&mut menu, &mut cx, OUTPUT, 0, Throw);
    assert_eq!(cx.thrown, [st("iron_ingot", 1)]);
    assert_eq!(own(&menu, OUTPUT), Some(st("iron_ingot", 3)));
}

// QUICK_CRAFT.

#[test]
fn a_left_drag_splits_the_stack_evenly_and_keeps_the_rest() {
    let mut rig = rig();
    let mut menu = chest(&[]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("stone", 10));
    drag_over(&mut menu, &mut cx, 0, &[0, 1, 2]);
    for slot in 0..3 {
        assert_eq!(own(&menu, slot), Some(st("stone", 3)));
    }
    assert_eq!(cx.inventory.cursor, Some(st("stone", 1)));
}

#[test]
fn a_left_drag_counts_full_stacks_and_caps_partial_ones() {
    let mut rig = rig();
    let mut menu = chest(&[(1, st("stone", 64)), (2, st("stone", 60))]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("stone", 30));
    drag_over(&mut menu, &mut cx, 0, &[0, 1, 2]);
    // Three slots, so 10 each: the full stack takes none and the other 4.
    assert_eq!(own(&menu, 0), Some(st("stone", 10)));
    assert_eq!(own(&menu, 1), Some(st("stone", 64)));
    assert_eq!(own(&menu, 2), Some(st("stone", 64)));
    assert_eq!(cx.inventory.cursor, Some(st("stone", 16)));
}

#[test]
fn a_drag_covers_no_more_slots_than_items() {
    let mut rig = rig();
    let mut menu = chest(&[]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("stone", 2));
    drag_over(&mut menu, &mut cx, 0, &[0, 1, 2]);
    assert_eq!(own(&menu, 0), Some(st("stone", 1)));
    assert_eq!(own(&menu, 1), Some(st("stone", 1)));
    assert_eq!(own(&menu, 2), None);
    assert_eq!(cx.inventory.cursor, None);
}

#[test]
fn a_drag_skips_other_items_and_counts_a_slot_once() {
    let mut rig = rig();
    let mut menu = chest(&[(0, st("dirt", 5))]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("stone", 8));
    drag_over(&mut menu, &mut cx, 0, &[0, 1, 2, 1, 0, 2]);
    assert_eq!(own(&menu, 0), Some(st("dirt", 5)));
    assert_eq!(own(&menu, 1), Some(st("stone", 4)));
    assert_eq!(own(&menu, 2), Some(st("stone", 4)));
    assert_eq!(cx.inventory.cursor, None);
}

#[test]
fn a_right_drag_places_one_in_each_slot() {
    let mut rig = rig();
    let mut menu = chest(&[(2, st("stone", 63))]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("stone", 5));
    drag_over(&mut menu, &mut cx, 1, &[0, 1, 2]);
    assert_eq!(own(&menu, 0), Some(st("stone", 1)));
    assert_eq!(own(&menu, 1), Some(st("stone", 1)));
    assert_eq!(own(&menu, 2), Some(st("stone", 64)));
    assert_eq!(cx.inventory.cursor, Some(st("stone", 2)));
}

#[test]
fn a_full_stack_drag_needs_creative_and_leaves_nothing_carried() {
    let mut rig = rig();
    let mut menu = chest(&[]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("stone", 5));
    drag_over(&mut menu, &mut cx, 2, &[0, 1]);
    assert_eq!(own(&menu, 0), None);
    assert_eq!(cx.inventory.cursor, Some(st("stone", 5)));

    cx.creative = true;
    drag_over(&mut menu, &mut cx, 2, &[0, 1]);
    assert_eq!(own(&menu, 0), Some(st("stone", 64)));
    assert_eq!(own(&menu, 1), Some(st("stone", 64)));
    // 5 - 128: the cursor ends empty.
    assert_eq!(cx.inventory.cursor, None);
}

#[test]
fn a_drag_over_one_slot_is_a_click_with_the_drag_type_as_button() {
    let mut rig = rig();
    let mut menu = chest(&[]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("stone", 10));
    drag_over(&mut menu, &mut cx, 1, &[3]);
    assert_eq!(own(&menu, 3), Some(st("stone", 1)));
    assert_eq!(cx.inventory.cursor, Some(st("stone", 9)));
    drag_over(&mut menu, &mut cx, 0, &[4]);
    assert_eq!(own(&menu, 4), Some(st("stone", 9)));
    assert_eq!(cx.inventory.cursor, None);
    // Button 2 is no click.
    cx.creative = true;
    cx.inventory.cursor = Some(st("stone", 10));
    drag_over(&mut menu, &mut cx, 2, &[5]);
    assert_eq!(own(&menu, 5), None);
    assert_eq!(cx.inventory.cursor, Some(st("stone", 10)));
    // Nothing carried, or an unknown type: nothing.
    drag_over(&mut menu, &mut cx, 3, &[5, 6]);
    cx.inventory.cursor = None;
    drag_over(&mut menu, &mut cx, 0, &[5, 6]);
    assert_eq!(own(&menu, 5), None);
    assert_eq!(own(&menu, 6), None);
}

#[test]
fn a_drag_respects_each_slot_and_what_it_takes() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("glass_bottle", 10));
    drag_over(&mut menu, &mut cx, 0, &[0, 3, PLAYER]);
    assert_eq!(own(&menu, 0), Some(st("glass_bottle", 1)));
    assert_eq!(own(&menu, 3), Some(st("glass_bottle", 3)));
    assert_eq!(inv(&cx, 9), Some(st("glass_bottle", 3)));
    assert_eq!(cx.inventory.cursor, Some(st("glass_bottle", 3)));

    // The output never joins, so this is a click on slot 3 alone.
    cx.inventory.cursor = Some(st("glass_bottle", 6));
    drag_over(&mut menu, &mut cx, 0, &[OUTPUT, 3]);
    assert_eq!(own(&menu, OUTPUT), None);
    assert_eq!(own(&menu, 3), Some(st("glass_bottle", 9)));
    assert_eq!(cx.inventory.cursor, None);
}

// PICKUP_ALL.

#[test]
fn a_double_click_gathers_partial_stacks_in_menu_order_first() {
    let mut rig = rig();
    let mut menu = chest(&[(3, st("stone", 64)), (7, st("stone", 10))]);
    let mut cx = rig.cx();
    cx.inventory.slots[9] = Some(st("stone", 20));
    cx.inventory.slots[0] = Some(st("stone", 40));
    cx.inventory.cursor = Some(st("stone", 5));
    press(&mut menu, &mut cx, 0, 0, PickupAll);
    // Chest slot 7, main 9, then hotbar 0 until the cursor is full.
    assert_eq!(own(&menu, 7), None);
    assert_eq!(inv(&cx, 9), None);
    assert_eq!(inv(&cx, 0), Some(st("stone", 11)));
    assert_eq!(own(&menu, 3), Some(st("stone", 64)));
    assert_eq!(cx.inventory.cursor, Some(st("stone", 64)));
}

#[test]
fn a_double_click_takes_from_full_stacks_in_its_second_pass() {
    let mut rig = rig();
    let mut menu = chest(&[(3, st("stone", 64))]);
    let mut cx = rig.cx();
    cx.inventory.slots[0] = Some(st("stone", 64));
    cx.inventory.cursor = Some(st("stone", 1));
    press(&mut menu, &mut cx, 0, 0, PickupAll);
    assert_eq!(own(&menu, 3), Some(st("stone", 1)));
    assert_eq!(inv(&cx, 0), Some(st("stone", 64)));
    assert_eq!(cx.inventory.cursor, Some(st("stone", 64)));
}

#[test]
fn a_double_click_with_button_one_scans_downward() {
    for (button, chest_left, hotbar_left) in [
        (0, None, Some(st("stone", 6))),
        (1, Some(st("stone", 6)), None),
    ] {
        let mut rig = rig();
        let mut menu = chest(&[(0, st("stone", 30))]);
        let mut cx = rig.cx();
        cx.inventory.slots[8] = Some(st("stone", 30));
        cx.inventory.cursor = Some(st("stone", 10));
        press(&mut menu, &mut cx, 5, button, PickupAll);
        assert_eq!(own(&menu, 0), chest_left);
        assert_eq!(inv(&cx, 8), hotbar_left);
        assert_eq!(cx.inventory.cursor, Some(st("stone", 64)));
    }
}

#[test]
fn a_double_click_needs_an_empty_or_locked_slot() {
    let mut rig = rig();
    let mut menu = chest(&[(0, st("stone", 5)), (1, st("stone", 5))]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("stone", 1));
    press(&mut menu, &mut cx, 0, 0, PickupAll);
    assert_eq!(own(&menu, 1), Some(st("stone", 5)));
    assert_eq!(cx.inventory.cursor, Some(st("stone", 1)));
    // Nothing carried: nothing.
    cx.inventory.cursor = None;
    press(&mut menu, &mut cx, 2, 0, PickupAll);
    assert_eq!(own(&menu, 1), Some(st("stone", 5)));

    let mut rig = self::rig();
    let mut menu = TestMenu::new(&[(LOCKED, st("dirt", 1)), (3, st("stone", 5))]);
    let mut cx = rig.cx();
    cx.inventory.cursor = Some(st("stone", 1));
    press(&mut menu, &mut cx, LOCKED, 0, PickupAll);
    assert_eq!(own(&menu, 3), None);
    assert_eq!(cx.inventory.cursor, Some(st("stone", 6)));
}

#[test]
fn a_double_click_skips_outputs_locked_slots_and_other_components() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[
        (OUTPUT, st("iron_ingot", 5)),
        (LOCKED, st("iron_ingot", 5)),
        (3, st("iron_ingot", 5)),
    ]);
    let mut cx = rig.cx();
    cx.inventory.slots[9] = Some(renamed("iron_ingot", 5));
    cx.inventory.cursor = Some(st("iron_ingot", 1));
    press(&mut menu, &mut cx, 0, 0, PickupAll);
    assert_eq!(own(&menu, OUTPUT), Some(st("iron_ingot", 5)));
    assert_eq!(own(&menu, LOCKED), Some(st("iron_ingot", 5)));
    assert_eq!(own(&menu, 3), None);
    assert_eq!(inv(&cx, 9), Some(renamed("iron_ingot", 5)));
    assert_eq!(cx.inventory.cursor, Some(st("iron_ingot", 6)));
}

// `removed`.

#[test]
fn closing_returns_the_cursor_to_the_selected_slot_first() {
    let mut rig = rig();
    let mut menu = chest(&[(0, st("dirt", 3))]);
    rig.inventory.slots[3] = Some(st("stone", 50));
    rig.inventory.slots[40] = Some(st("stone", 60));
    rig.inventory.slots[2] = Some(st("stone", 63));
    rig.inventory.slots[1] = Some(st("dirt", 1));
    rig.inventory.cursor = Some(st("stone", 30));
    let mut cx = rig.cx();
    cx.selected = 3;
    assert!(handle(&mut menu, &mut cx, &MenuInput::Close));
    // 14 to the selected slot, 4 to the offhand, 1 to slot 2, the last 11
    // to the first free slot.
    assert_eq!(inv(&cx, 3), Some(st("stone", 64)));
    assert_eq!(inv(&cx, 40), Some(st("stone", 64)));
    assert_eq!(inv(&cx, 2), Some(st("stone", 64)));
    assert_eq!(inv(&cx, 0), Some(st("stone", 11)));
    assert_eq!(cx.inventory.cursor, None);
    assert!(cx.thrown.is_empty());
    // The chest keeps its items.
    assert_eq!(own(&menu, 0), Some(st("dirt", 3)));
    assert!(menu.own_mut().take_changed().is_empty());
}

#[test]
fn closing_drops_what_does_not_fit_without_counting_it() {
    let mut rig = rig();
    fill_inventory(&mut rig.inventory, &st("dirt", 64));
    rig.inventory.slots[4] = Some(st("stone", 60));
    rig.inventory.cursor = Some(st("stone", 10));
    let mut menu = chest(&[]);
    let mut cx = rig.cx();
    assert!(handle(&mut menu, &mut cx, &MenuInput::Close));
    assert_eq!(inv(&cx, 4), Some(st("stone", 64)));
    assert_eq!(cx.thrown, [st("stone", 6)]);
    assert!(cx.inventory.take_stat_events().is_empty());
}

#[test]
fn closing_puts_an_unstackable_item_in_the_first_free_slot() {
    let mut rig = rig();
    rig.inventory.slots[0] = Some(st("diamond_sword", 1));
    rig.inventory.cursor = Some(st("diamond_sword", 1));
    let mut menu = HopperMenu::new(Vec::new());
    let mut cx = rig.cx();
    assert!(handle(&mut menu, &mut cx, &MenuInput::Close));
    assert_eq!(inv(&cx, 1), Some(st("diamond_sword", 1)));
    assert_eq!(cx.inventory.cursor, None);
}

#[test]
fn clearing_own_slots_returns_them_to_the_inventory() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[(3, st("stone", 5)), (OUTPUT, st("iron_ingot", 2))]);
    let mut cx = rig.cx();
    clear_own_slots(&mut menu, &mut cx, 0..4);
    assert_eq!(own(&menu, 3), None);
    assert_eq!(own(&menu, OUTPUT), Some(st("iron_ingot", 2)));
    assert_eq!(inv(&cx, 0), Some(st("stone", 5)));
    // `removeItemNoUpdate`: the menu is not told.
    assert!(menu.changed.is_empty());
}

// The shulker box.

#[test]
fn shulker_boxes_do_not_fit_inside_container_items() {
    assert!(!fits_inside_container_items("minecraft:shulker_box"));
    for colour in super::storage::DYE_COLOURS {
        assert!(!fits_inside_container_items(&format!(
            "minecraft:{colour}_shulker_box"
        )));
    }
    for id in [
        "minecraft:shulker_shell",
        "minecraft:chest",
        "minecraft:bundle",
        "minecraft:barrel",
        "minecraft:stone",
    ] {
        assert!(fits_inside_container_items(id), "{id}");
    }
}

#[test]
fn a_shulker_box_refuses_shulker_boxes_every_way() {
    let mut rig = rig();
    let mut menu = ShulkerBoxMenu::new(Vec::new());
    let mut cx = rig.cx();
    for id in ["shulker_box", "red_shulker_box"] {
        cx.inventory.cursor = Some(st(id, 1));
        press(&mut menu, &mut cx, 0, 0, Pickup);
        press(&mut menu, &mut cx, 0, 1, Pickup);
        drag_over(&mut menu, &mut cx, 1, &[0, 1]);
        assert_eq!(own(&menu, 0), None);
        assert_eq!(cx.inventory.cursor, Some(st(id, 1)));
    }
    cx.inventory.cursor = None;
    cx.inventory.slots[0] = Some(st("white_shulker_box", 1));
    press(&mut menu, &mut cx, 0, 0, Swap);
    press(&mut menu, &mut cx, player_slot(27, 0), 0, QuickMove);
    assert!(menu.own().items().iter().all(Option::is_none));
    assert_eq!(inv(&cx, 0), Some(st("white_shulker_box", 1)));

    // Other items go in; the player's own slots take shulker boxes.
    cx.inventory.cursor = Some(st("shulker_shell", 5));
    press(&mut menu, &mut cx, 0, 0, Pickup);
    assert_eq!(own(&menu, 0), Some(st("shulker_shell", 5)));
    cx.inventory.cursor = Some(st("shulker_box", 1));
    press(&mut menu, &mut cx, player_slot(27, 9), 0, Pickup);
    assert_eq!(inv(&cx, 9), Some(st("shulker_box", 1)));
}

#[test]
fn a_shulker_box_already_inside_can_still_be_taken_out() {
    let mut rig = rig();
    let mut menu = ShulkerBoxMenu::new(grid(
        27,
        &[(0, st("shulker_box", 1)), (1, st("shulker_box", 1))],
    ));
    let mut cx = rig.cx();
    press(&mut menu, &mut cx, 0, 0, Pickup);
    assert_eq!(cx.inventory.cursor, Some(st("shulker_box", 1)));
    press(&mut menu, &mut cx, 1, 0, QuickMove);
    assert_eq!(inv(&cx, 8), Some(st("shulker_box", 1)));
    assert!(menu.own().items().iter().all(Option::is_none));
}

// The hopper and the dispenser.

#[test]
fn a_hopper_shift_clicks_between_its_row_and_the_inventory() {
    let mut rig = rig();
    let mut menu = HopperMenu::new(grid(5, &[(0, st("stone", 10)), (2, st("dirt", 60))]));
    let mut cx = rig.cx();
    press(&mut menu, &mut cx, 0, 0, QuickMove);
    assert_eq!(own(&menu, 0), None);
    assert_eq!(inv(&cx, 8), Some(st("stone", 10)));
    assert_eq!(menu.slots()[40].at, SlotRef::Player(8));

    cx.inventory.slots[9] = Some(st("dirt", 10));
    press(&mut menu, &mut cx, player_slot(5, 9), 0, QuickMove);
    assert_eq!(own(&menu, 2), Some(st("dirt", 64)));
    assert_eq!(own(&menu, 0), Some(st("dirt", 6)));
    assert_eq!(inv(&cx, 9), None);
}

#[test]
fn a_dispenser_shift_clicks_between_its_grid_and_the_inventory() {
    let mut rig = rig();
    let mut menu = DispenserMenu::new(grid(9, &[(4, st("arrow", 10))]));
    let mut cx = rig.cx();
    assert_eq!(menu.quick_move_stack(&mut cx, 4), Some(st("arrow", 10)));
    assert_eq!(own(&menu, 4), None);
    assert_eq!(inv(&cx, 8), Some(st("arrow", 10)));

    cx.inventory.slots[0] = Some(st("arrow", 10));
    press(&mut menu, &mut cx, player_slot(9, 0), 0, QuickMove);
    assert_eq!(own(&menu, 0), Some(st("arrow", 10)));
    assert_eq!(inv(&cx, 0), None);

    // Full of something else: nothing moves, and nothing is reported.
    let full: Vec<(usize, ItemStack)> = (0..9).map(|index| (index, st("dirt", 64))).collect();
    let mut menu = DispenserMenu::new(grid(9, &full));
    cx.inventory.slots[0] = Some(st("arrow", 10));
    assert_eq!(menu.quick_move_stack(&mut cx, player_slot(9, 0)), None);
    assert_eq!(inv(&cx, 0), Some(st("arrow", 10)));
}

// Change tracking and the other inputs.

#[test]
fn own_slot_changes_are_reported_once() {
    let mut rig = rig();
    let mut menu = chest(&[]);
    menu.own_mut()
        .load(grid(27, &[(0, st("stone", 5)), (4, st("dirt", 1))]));
    assert!(menu.own_mut().take_changed().is_empty());
    let mut cx = rig.cx();
    press(&mut menu, &mut cx, 0, 0, Pickup);
    assert_eq!(menu.own_mut().take_changed(), [0]);
    assert!(menu.own_mut().take_changed().is_empty());
    press(&mut menu, &mut cx, 5, 0, Pickup);
    press(&mut menu, &mut cx, 4, 2, Swap);
    assert_eq!(menu.own().changed().collect::<Vec<_>>(), [4, 5]);
    assert!(menu.own().is_changed(4));
    assert!(!menu.own().is_changed(0));
    // Clicking an empty slot with nothing carried writes nothing.
    menu.own_mut().take_changed();
    press(&mut menu, &mut cx, 9, 0, Pickup);
    assert!(menu.own_mut().take_changed().is_empty());
    assert_eq!(cx.player_slots_written().collect::<Vec<_>>(), [2]);
}

#[test]
fn own_slot_writes_tell_the_menu_and_player_slot_writes_do_not() {
    let mut rig = rig();
    let mut menu = TestMenu::new(&[(3, st("stone", 5))]);
    let mut cx = rig.cx();
    press(&mut menu, &mut cx, 3, 0, Pickup);
    // `tryRemove`'s `removeItem` and `setByPlayer(EMPTY)`, `onTake` and the
    // click's closing `setChanged` all mark the container changed.
    assert!(menu.changed.len() >= 4);
    assert!(menu.changed.iter().all(|&slot| slot == 3));
    menu.changed.clear();
    press(&mut menu, &mut cx, PLAYER, 0, Pickup);
    assert!(menu.changed.is_empty());
}

#[test]
fn menu_packets_a_storage_menu_has_no_use_for_change_nothing() {
    let mut rig = rig();
    let mut menu = chest(&[(0, st("stone", 5))]);
    let mut cx = rig.cx();
    assert!(!handle(&mut menu, &mut cx, &MenuInput::Button(0)));
    assert!(!handle(
        &mut menu,
        &mut cx,
        &MenuInput::SetBeacon {
            primary: Some("minecraft:speed".into()),
            secondary: None
        }
    ));
    assert!(handle(&mut menu, &mut cx, &MenuInput::Rename("Box".into())));
    assert!(handle(&mut menu, &mut cx, &MenuInput::SelectTrade(1)));
    assert!(handle(
        &mut menu,
        &mut cx,
        &MenuInput::SlotState {
            slot: 0,
            enabled: false
        }
    ));
    assert_eq!(own(&menu, 0), Some(st("stone", 5)));
    assert!(menu.own_mut().take_changed().is_empty());
    assert!(menu.data().is_empty());
}

#[test]
fn inventory_add_fills_partial_stacks_then_a_free_slot() {
    let mut rig = rig();
    rig.inventory.slots[2] = Some(st("stone", 60));
    rig.inventory.slots[40] = Some(st("stone", 62));
    let mut cx = rig.cx();
    cx.selected = 2;
    let mut stack = st("stone", 10);
    assert!(add_to_inventory(&mut cx, &mut stack));
    assert_eq!(stack.count, 0);
    assert_eq!(inv(&cx, 2), Some(st("stone", 64)));
    assert_eq!(inv(&cx, 40), Some(st("stone", 64)));
    assert_eq!(inv(&cx, 0), Some(st("stone", 4)));

    // What does not fit stays, and the call says so; creative destroys it.
    for creative in [false, true] {
        let mut rig = self::rig();
        fill_inventory(&mut rig.inventory, &st("dirt", 64));
        rig.inventory.slots[5] = Some(st("stone", 60));
        let mut cx = rig.cx();
        cx.creative = creative;
        let mut stack = st("stone", 10);
        assert_eq!(add_to_inventory(&mut cx, &mut stack), creative);
        assert_eq!(stack.count, if creative { 0 } else { 6 });
        assert_eq!(inv(&cx, 5), Some(st("stone", 64)));
    }
}

#[test]
fn quick_replace_and_stackable_follow_vanilla() {
    let stone = st("stone", 10);
    assert!(can_item_quick_replace(None, &stone, false));
    assert!(can_item_quick_replace(
        Some(&st("stone", 54)),
        &stone,
        false
    ));
    assert!(!can_item_quick_replace(
        Some(&st("stone", 55)),
        &stone,
        false
    ));
    assert!(can_item_quick_replace(Some(&st("stone", 64)), &stone, true));
    assert!(!can_item_quick_replace(Some(&st("dirt", 1)), &stone, true));
    assert!(!can_item_quick_replace(
        Some(&renamed("stone", 1)),
        &stone,
        true
    ));
    assert!(is_stackable(&stone));
    assert!(!is_stackable(&st("diamond_sword", 1)));
}
