//! `AbstractContainerMenu` and `Slot` (26.3): the one click engine every
//! container screen runs on. A menu lists its slots in vanilla menu order,
//! each either one of the player's inventory slots or one of the menu's own
//! slots (a block's storage, or a workstation's inputs and result). The
//! engine applies what a screen sends (`ServerboundContainerClickPacket` and
//! the other menu packets) by `doClick`'s rules, and a [`Menu`] supplies what
//! vanilla's menu and `Slot` subclasses override.
//!
//! The menu owns its own slots ([`OwnSlots`]), so a server can load them from
//! block storage before applying inputs and write back the ones that changed.
//! What the menu needs of the player comes in a [`MenuContext`], which also
//! collects what the inputs produce for the world: stacks thrown, levels
//! spent, experience to award, and the level events and sounds at the
//! menu's block.
//!
//! Not ported: bundles' click overrides (`tryItemClickBehaviourOverride`),
//! which no menu here needs yet.
use crate::{
    inventory::{Inventory, ItemStack},
    rng::LegacyRandom,
};
use std::collections::BTreeSet;

pub mod storage;
#[cfg(test)]
mod tests;

pub use storage::{ChestMenu, DispenserMenu, HopperMenu, ShulkerBoxMenu};

/// `AbstractContainerMenu.SLOT_CLICKED_OUTSIDE`: a click outside the window.
pub const SLOT_CLICKED_OUTSIDE: i32 = -999;

/// `Container.getMaxStackSize`'s default (`Item.ABSOLUTE_MAX_STACK_SIZE`).
/// A slot holds at most this many, or its item's maximum if that is less.
pub const CONTAINER_MAX_STACK: i32 = 99;

/// The `SWAP` button for the offhand (`Inventory.SLOT_OFFHAND`). Buttons 0-8
/// are the hotbar.
pub const OFFHAND: i32 = 40;

/// `ContainerInput` (`ClickType` before 26.3), less `QUICK_CRAFT`, which
/// arrives whole as [`MenuInput::Drag`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContainerInput {
    /// A plain click: button 0 (primary) or 1 (secondary).
    Pickup,
    /// A shift-click: button 0 or 1.
    QuickMove,
    /// A hotbar key (button 0-8) or the offhand key (button 40).
    Swap,
    /// A pick-block click, in creative.
    Clone,
    /// The drop key: button 0 drops one, button 1 the stack.
    Throw,
    /// A double click: button 0 scans the slots upward, others downward.
    PickupAll,
}

/// One input from a container screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MenuInput {
    /// `ServerboundContainerClickPacket`. `slot` is a menu slot index, or
    /// [`SLOT_CLICKED_OUTSIDE`]; `button` is as [`ContainerInput`] says.
    Click {
        slot: i32,
        button: i32,
        kind: ContainerInput,
    },
    /// A whole `QUICK_CRAFT` drag: its start, one add per slot in the order
    /// the mouse passed them, and its end. `button` is the drag type: 0
    /// splits the carried stack evenly, 1 places one each, 2 places full
    /// stacks (creative).
    Drag { button: i32, slots: Vec<usize> },
    /// `ServerboundContainerButtonClickPacket` (`clickMenuButton`).
    Button(i32),
    /// `ServerboundRenameItemPacket` (the anvil).
    Rename(String),
    /// `ServerboundSelectTradePacket` (villager trading).
    SelectTrade(i32),
    /// `ServerboundSetBeaconPacket`: the chosen effects' ids.
    SetBeacon {
        primary: Option<String>,
        secondary: Option<String>,
    },
    /// `ServerboundContainerSlotStateChangedPacket` (the crafter).
    SlotState { slot: usize, enabled: bool },
    /// `ServerboundContainerClosePacket`: the menu's `removed`.
    Close,
}

/// Where a menu's effect in the world takes place.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuPlace {
    /// At the player (`player.position()`): a furnace's output experience
    /// (`AbstractFurnaceBlockEntity.awardUsedRecipesAndPopExperience`).
    Player,
    /// At the centre of the menu's block, the position of its
    /// `ContainerLevelAccess` (`Vec3.atCenterOf(pos)`): the grindstone's
    /// experience.
    Block,
}

/// What a menu slot shows and changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotRef {
    /// An index into the player's `Inventory.slots`: 0-8 hotbar, 9-35 main,
    /// 36-39 armour (feet to head), 40 offhand.
    Player(usize),
    /// An index into the menu's [`OwnSlots`].
    Own(usize),
}

/// One `Slot` of a menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotDef {
    pub at: SlotRef,
    /// `Slot.x` and `Slot.y`: the item's top-left corner, from the screen
    /// image's top-left (`leftPos`, `topPos`).
    pub x: i32,
    pub y: i32,
    /// `Slot.getNoItemIcon`: the sprite an empty slot shows.
    pub icon: Option<&'static str>,
}

impl SlotDef {
    pub const fn own(index: usize, x: i32, y: i32) -> Self {
        Self {
            at: SlotRef::Own(index),
            x,
            y,
            icon: None,
        }
    }

    pub const fn player(index: usize, x: i32, y: i32) -> Self {
        Self {
            at: SlotRef::Player(index),
            x,
            y,
            icon: None,
        }
    }

    pub const fn with_icon(self, icon: &'static str) -> Self {
        Self {
            icon: Some(icon),
            ..self
        }
    }
}

/// `AbstractContainerMenu.addStandardInventorySlots`: the 27 main slots
/// (inventory 9-35) in rows from `top`, then the hotbar (0-8) 58 pixels
/// lower. Every container menu lists these after its own slots.
pub fn standard_inventory_slots(left: i32, top: i32) -> Vec<SlotDef> {
    let main = (0..3).flat_map(|y| {
        (0..9).map(move |x| {
            SlotDef::player(x + (y + 1) * 9, left + x as i32 * 18, top + y as i32 * 18)
        })
    });
    let hotbar = (0..9).map(|x| SlotDef::player(x, left + x as i32 * 18, top + 58));
    main.chain(hotbar).collect()
}

/// A menu's own slots, with a note of which were written since the owner
/// last asked ([`OwnSlots::take_changed`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OwnSlots {
    items: Vec<Option<ItemStack>>,
    changed: Vec<bool>,
}

impl OwnSlots {
    pub fn new(items: Vec<Option<ItemStack>>) -> Self {
        let changed = vec![false; items.len()];
        Self { items, changed }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[Option<ItemStack>] {
        &self.items
    }

    pub fn get(&self, index: usize) -> Option<&ItemStack> {
        self.items.get(index).and_then(Option::as_ref)
    }

    /// Write a slot, noting it changed if the stack differs.
    pub fn set(&mut self, index: usize, stack: Option<ItemStack>) {
        let Some(slot) = self.items.get_mut(index) else {
            return;
        };
        if *slot != stack {
            *slot = stack;
            self.changed[index] = true;
        }
    }

    /// Replace every slot (from block storage, say) without noting changes.
    pub fn load(&mut self, items: Vec<Option<ItemStack>>) {
        self.changed = vec![false; items.len()];
        self.items = items;
    }

    pub fn is_changed(&self, index: usize) -> bool {
        self.changed.get(index).copied().unwrap_or(false)
    }

    /// The slots written since the last [`OwnSlots::take_changed`], in order.
    pub fn changed(&self) -> impl Iterator<Item = usize> + '_ {
        self.changed
            .iter()
            .enumerate()
            .filter(|(_, changed)| **changed)
            .map(|(index, _)| index)
    }

    /// The slots written since the last call, in order; forgets them.
    pub fn take_changed(&mut self) -> Vec<usize> {
        let changed = self.changed().collect();
        self.changed.iter_mut().for_each(|changed| *changed = false);
        changed
    }

    pub fn into_items(self) -> Vec<Option<ItemStack>> {
        self.items
    }
}

/// The player a menu works for: what it may read and change of them, and
/// what its inputs produce for the world.
pub struct MenuContext<'a> {
    /// The player's slots and the carried stack (`Inventory.cursor`).
    pub inventory: &'a mut Inventory,
    /// `Inventory.selected`, the hotbar slot in hand: returned and added
    /// stacks go there first.
    pub selected: usize,
    /// `Player.hasInfiniteMaterials`: creative mode.
    pub creative: bool,
    /// `Player.experienceLevel`.
    pub xp_level: i32,
    /// `Player.enchantmentSeed`, which the enchanting table's offers come
    /// from. A menu that re-rolls it (`onEnchantmentPerformed`) writes the
    /// new seed here, for the player to keep.
    pub enchantment_seed: i32,
    /// The randomness menus draw on.
    pub random: &'a mut LegacyRandom,
    /// Stacks to throw from the player (`Player.drop`), in order.
    pub thrown: Vec<ItemStack>,
    /// Levels the inputs spent (enchanting, the anvil).
    pub xp_levels_spent: i32,
    /// Experience to award as orbs, one `ExperienceOrb.award` per entry,
    /// where each says.
    pub xp_orbs: Vec<(MenuPlace, i32)>,
    /// `Level.levelEvent(id, pos, 0)` at the menu's block, through its
    /// access: the anvil's use (1030) and break (1029), the grindstone's
    /// (1042), the smithing table's (1044).
    pub level_events: Vec<i32>,
    /// `Level.playSound(null, pos, event, BLOCKS, volume, pitch)` at the
    /// menu's block: the stonecutter's, loom's and cartography table's take
    /// sounds, the enchanting table's use. (event, volume, pitch).
    pub sounds: Vec<(&'static str, f32, f32)>,
    /// Player slots written during the current input, each with its stack
    /// before the first write.
    touched: Vec<(usize, Option<ItemStack>)>,
    /// Every player slot written through this context.
    written: BTreeSet<usize>,
}

impl<'a> MenuContext<'a> {
    /// A survival player at level 0 with hotbar slot 0 selected.
    pub fn new(inventory: &'a mut Inventory, random: &'a mut LegacyRandom) -> Self {
        Self {
            inventory,
            selected: 0,
            creative: false,
            xp_level: 0,
            enchantment_seed: 0,
            random,
            thrown: Vec::new(),
            xp_levels_spent: 0,
            xp_orbs: Vec::new(),
            level_events: Vec::new(),
            sounds: Vec::new(),
            touched: Vec::new(),
            written: BTreeSet::new(),
        }
    }

    /// The player slots the inputs wrote, in order. Compare them with a
    /// snapshot for the net change.
    pub fn player_slots_written(&self) -> impl Iterator<Item = usize> + '_ {
        self.written.iter().copied()
    }

    /// Write a player inventory slot.
    pub fn set_player(&mut self, index: usize, stack: Option<ItemStack>) {
        let Some(slot) = self.inventory.slots.get_mut(index) else {
            return;
        };
        if !self.touched.iter().any(|(touched, _)| *touched == index) {
            self.touched.push((index, slot.clone()));
        }
        self.written.insert(index);
        *slot = stack;
    }

    /// `ServerPlayer`'s menu listener: `InventoryChangeTrigger` for every
    /// player slot the last input changed.
    fn notice_changes(&mut self) {
        for (index, before) in std::mem::take(&mut self.touched) {
            if self.inventory.slots.get(index) != Some(&before) {
                self.inventory.notice_slot_after_change(index, before);
            }
        }
    }

    /// `Player.drop`: throw a stack from the player. A stack thrown from the
    /// hand (`thrownFromHand`) counts towards the dropped statistics.
    pub fn throw(&mut self, stack: ItemStack, from_hand: bool) {
        if stack.count == 0 {
            return;
        }
        if from_hand {
            self.inventory.record_dropped(&stack);
        }
        self.thrown.push(stack);
    }
}

/// What each kind of menu, and each kind of `Slot` in it, decides. `slot` is
/// always a menu slot index. The defaults are those of a plain `Slot` and of
/// `AbstractContainerMenu`.
pub trait Menu {
    /// The vanilla `MenuType` id, e.g. `minecraft:generic_9x3`.
    fn menu_type(&self) -> &'static str;

    /// Every slot, in vanilla menu order.
    fn slots(&self) -> &[SlotDef];

    /// The stacks behind the [`SlotRef::Own`] slots.
    fn own(&self) -> &OwnSlots;

    /// The stacks behind the [`SlotRef::Own`] slots, to load or write.
    fn own_mut(&mut self) -> &mut OwnSlots;

    /// `Slot.mayPlace`. Vanilla never consults `Container.canPlaceItem`
    /// here; that governs hoppers.
    fn may_place(&self, _cx: &MenuContext, _slot: usize, _stack: &ItemStack) -> bool {
        true
    }

    /// `Slot.mayPickup`.
    fn may_pickup(&self, _cx: &MenuContext, _slot: usize) -> bool {
        true
    }

    /// `Slot.getMaxStackSize(stack)`: how many of `stack` the slot holds.
    fn max_stack(&self, _cx: &MenuContext, _slot: usize, stack: &ItemStack) -> i32 {
        CONTAINER_MAX_STACK.min(i32::from(stack.max))
    }

    /// `quickMoveStack`: move what `slot` holds elsewhere, returning a copy
    /// of the stack as it was, or `None` when nothing moved (which ends
    /// `QUICK_MOVE`'s repeat loop).
    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack>;

    /// `Slot.remove`: take up to `amount` from the slot's container
    /// (`Container.removeItem`). A `ResultContainer` gives its whole stack.
    fn remove(&mut self, cx: &mut MenuContext, slot: usize, amount: i32) -> Option<ItemStack> {
        remove_from(self, cx, slot, amount)
    }

    /// `Slot.onTake`: the player took `taken` from the slot. Result slots
    /// pay for it here (crafting, furnace experience, trades).
    fn on_take(&mut self, cx: &mut MenuContext, slot: usize, _taken: &ItemStack) {
        set_changed(self, cx, slot);
    }

    /// `Slot.onSwapCraft`: a hotbar key took `count` from the slot.
    fn on_swap_craft(&mut self, _cx: &mut MenuContext, _slot: usize, _count: i32) {}

    /// `AbstractContainerMenu.slotsChanged`, reached through
    /// `Container.setChanged`: the container behind own slot `slot` was
    /// marked changed. The engine calls it where vanilla's `Slot` does
    /// (`set`, `setChanged`, `onTake`, and a `removeItem` that took
    /// something). Workstations recompute their result here.
    fn slots_changed(&mut self, _cx: &mut MenuContext, _slot: usize) {}

    /// `clickMenuButton`: whether the button did anything.
    fn click_button(&mut self, _cx: &mut MenuContext, _id: i32) -> bool {
        false
    }

    /// `AnvilMenu.setItemName`.
    fn rename(&mut self, _cx: &mut MenuContext, _name: &str) {}

    /// `MerchantMenu.setSelectionHint` and `tryMoveItems`.
    fn select_trade(&mut self, _cx: &mut MenuContext, _index: i32) {}

    /// `BeaconMenu.updateEffects`: whether the choice was valid (vanilla
    /// disconnects a player who sends an invalid one).
    fn set_beacon(
        &mut self,
        _cx: &mut MenuContext,
        _primary: Option<&str>,
        _secondary: Option<&str>,
    ) -> bool {
        false
    }

    /// `CrafterMenu.setSlotState`.
    fn set_slot_state(&mut self, _cx: &mut MenuContext, _slot: usize, _enabled: bool) {}

    /// `removed`: the menu closes. The base returns the carried stack to the
    /// inventory; workstations also return their inputs (`clearContainer`).
    /// A block's storage keeps its items.
    fn removed(&mut self, cx: &mut MenuContext) {
        return_carried(cx);
    }

    /// The `ContainerData` values the screen draws from (progress bars,
    /// costs), in data-slot order.
    fn data(&self) -> Vec<i32> {
        Vec::new()
    }

    /// `canTakeItemForPickAll`: whether a double click may gather from
    /// `slot`. Menus with a result slot exclude it.
    fn can_take_for_pick_all(&self, _carried: &ItemStack, _slot: usize) -> bool {
        true
    }

    /// `canDragTo`: whether a drag may cover `slot`.
    fn can_drag_to(&self, _slot: usize) -> bool {
        true
    }

    /// Which of the menu's containers the own slot `slot` shows
    /// (`Slot.container`), numbered from 0. A storage menu has one; a
    /// workstation keeps its result in a container of its own
    /// (`CraftingMenu.resultSlots`, `ItemCombinerMenu.resultSlots`).
    fn own_container(&self, _slot: usize) -> usize {
        0
    }
}

/// `target.container == slot.container`: whether two menu slots show the
/// same container. The player's slots are all its `Inventory`; the menu's
/// own are in the containers [`Menu::own_container`] names.
pub fn same_container<M: Menu + ?Sized>(menu: &M, a: usize, b: usize) -> bool {
    let (Some(first), Some(second)) = (menu.slots().get(a), menu.slots().get(b)) else {
        return false;
    };
    match (first.at, second.at) {
        (SlotRef::Player(_), SlotRef::Player(_)) => true,
        (SlotRef::Own(_), SlotRef::Own(_)) => menu.own_container(a) == menu.own_container(b),
        _ => false,
    }
}

/// Apply one input: `ServerGamePacketListenerImpl`'s menu handlers, after
/// their `stillValid` check, which is the caller's. Returns false when the
/// menu refused it (a button it has no use for, an invalid beacon choice).
pub fn handle<M: Menu + ?Sized>(menu: &mut M, cx: &mut MenuContext, input: &MenuInput) -> bool {
    let accepted = match input {
        MenuInput::Click { slot, button, kind } => {
            click(menu, cx, *slot, *button, *kind);
            true
        }
        MenuInput::Drag { button, slots } => {
            drag(menu, cx, *button, slots);
            true
        }
        MenuInput::Button(id) => menu.click_button(cx, *id),
        MenuInput::Rename(name) => {
            menu.rename(cx, name);
            true
        }
        MenuInput::SelectTrade(index) => {
            menu.select_trade(cx, *index);
            true
        }
        MenuInput::SetBeacon { primary, secondary } => {
            menu.set_beacon(cx, primary.as_deref(), secondary.as_deref())
        }
        MenuInput::SlotState { slot, enabled } => {
            menu.set_slot_state(cx, *slot, *enabled);
            true
        }
        MenuInput::Close => {
            menu.removed(cx);
            true
        }
    };
    cx.notice_changes();
    accepted
}

/// `AbstractContainerMenu.doClick` for everything but `QUICK_CRAFT`.
pub fn click<M: Menu + ?Sized>(
    menu: &mut M,
    cx: &mut MenuContext,
    slot: i32,
    button: i32,
    kind: ContainerInput,
) {
    // `isValidSlotIndex`: the server ignores clicks on slots past the list.
    // Other negative slots reach `doClick`, where they do nothing (vanilla
    // would throw on some).
    if slot >= 0 && slot as usize >= menu.slots().len() {
        return;
    }
    let index = usize::try_from(slot).ok();
    match kind {
        ContainerInput::Pickup | ContainerInput::QuickMove if button == 0 || button == 1 => {
            let primary = button == 0;
            if slot == SLOT_CLICKED_OUTSIDE {
                drop_carried(cx, primary);
            } else if let Some(index) = index {
                if kind == ContainerInput::QuickMove {
                    quick_move(menu, cx, index);
                } else {
                    pickup(menu, cx, index, primary);
                }
            }
        }
        ContainerInput::Swap if (0..9).contains(&button) || button == OFFHAND => {
            if let Some(index) = index {
                swap(menu, cx, index, button as usize);
            }
        }
        ContainerInput::Clone if cx.creative && cx.inventory.cursor.is_none() => {
            // `Slot.safeClone`: a copy at the item's maximum.
            let copy = index
                .and_then(|index| item(menu, cx, index))
                .map(|stack| ItemStack {
                    count: stack.max,
                    ..stack.clone()
                });
            if copy.is_some() {
                cx.inventory.cursor = copy;
            }
        }
        ContainerInput::Throw if cx.inventory.cursor.is_none() => {
            if let Some(index) = index {
                throw(menu, cx, index, button);
            }
        }
        ContainerInput::PickupAll => {
            if let Some(index) = index {
                pickup_all(menu, cx, index, button);
            }
        }
        _ => {}
    }
}

/// A click outside the window drops the carried stack, or one of it.
fn drop_carried(cx: &mut MenuContext, primary: bool) {
    let Some(carried) = cx.inventory.cursor.as_mut() else {
        return;
    };
    let dropped = if primary {
        cx.inventory.cursor.take()
    } else {
        let one = split(carried, 1);
        if carried.count == 0 {
            cx.inventory.cursor = None;
        }
        Some(one)
    };
    if let Some(dropped) = dropped {
        cx.throw(dropped, true);
    }
}

/// `PICKUP` on a slot.
fn pickup<M: Menu + ?Sized>(menu: &mut M, cx: &mut MenuContext, index: usize, primary: bool) {
    let clicked = item(menu, cx, index).cloned();
    let carried = cx.inventory.cursor.clone();
    match (clicked, carried) {
        (None, None) => {}
        (None, Some(carried)) => {
            let amount = if primary { i32::from(carried.count) } else { 1 };
            let rest = safe_insert(menu, cx, index, carried, amount);
            cx.inventory.cursor = rest;
        }
        (Some(clicked), carried) if menu.may_pickup(cx, index) => match carried {
            None => {
                let count = i32::from(clicked.count);
                let amount = if primary { count } else { (count + 1) / 2 };
                if let Some(taken) = try_remove(menu, cx, index, amount, i32::MAX) {
                    cx.inventory.cursor = Some(taken.clone());
                    menu.on_take(cx, index, &taken);
                }
            }
            Some(carried) if menu.may_place(cx, index, &carried) => {
                if clicked.same_item(&carried) {
                    let amount = if primary { i32::from(carried.count) } else { 1 };
                    let rest = safe_insert(menu, cx, index, carried, amount);
                    cx.inventory.cursor = rest;
                } else if i32::from(carried.count) <= menu.max_stack(cx, index, &carried) {
                    cx.inventory.cursor = Some(clicked);
                    set(menu, cx, index, Some(carried));
                }
            }
            // A slot that takes nothing (a result) still gives onto a
            // matching cursor, but only the whole stack.
            Some(mut carried) if clicked.same_item(&carried) => {
                let room = i32::from(carried.max) - i32::from(carried.count);
                if let Some(taken) = try_remove(menu, cx, index, i32::from(clicked.count), room) {
                    carried.count += taken.count;
                    cx.inventory.cursor = Some(carried);
                    menu.on_take(cx, index, &taken);
                }
            }
            Some(_) => {}
        },
        (Some(_), _) => {}
    }
    set_changed(menu, cx, index);
}

/// `QUICK_MOVE`: `quickMoveStack`, again while it moved something and the
/// slot still holds that item (a result refilling, or a stack moved one
/// slot's worth at a time).
fn quick_move<M: Menu + ?Sized>(menu: &mut M, cx: &mut MenuContext, index: usize) {
    if !menu.may_pickup(cx, index) {
        return;
    }
    let mut moved = menu.quick_move_stack(cx, index);
    while let Some(previous) = moved {
        // `ItemStack.isSameItem`: the item alone, not its components.
        if !item(menu, cx, index).is_some_and(|stack| stack.id == previous.id) {
            break;
        }
        moved = menu.quick_move_stack(cx, index);
    }
}

/// `SWAP` between a slot and an inventory slot (0-8 hotbar, 40 offhand).
fn swap<M: Menu + ?Sized>(menu: &mut M, cx: &mut MenuContext, index: usize, button: usize) {
    let source = cx.inventory.slots.get(button).cloned().flatten();
    let target = item(menu, cx, index).cloned();
    match (source, target) {
        (None, None) => {}
        (None, Some(target)) => {
            if menu.may_pickup(cx, index) {
                cx.set_player(button, Some(target.clone()));
                menu.on_swap_craft(cx, index, i32::from(target.count));
                set(menu, cx, index, None);
                menu.on_take(cx, index, &target);
            }
        }
        (Some(mut source), None) => {
            if menu.may_place(cx, index, &source) {
                let max = menu.max_stack(cx, index, &source);
                if i32::from(source.count) > max {
                    let part = split(&mut source, max);
                    cx.set_player(button, Some(source));
                    set(menu, cx, index, Some(part));
                } else {
                    cx.set_player(button, None);
                    set(menu, cx, index, Some(source));
                }
            }
        }
        (Some(mut source), Some(mut target)) => {
            if menu.may_pickup(cx, index) && menu.may_place(cx, index, &source) {
                let max = menu.max_stack(cx, index, &source);
                if i32::from(source.count) > max {
                    // The slot takes what it can hold; what it held goes into
                    // the inventory, or is dropped if it does not fit.
                    let part = split(&mut source, max);
                    cx.set_player(button, Some(source));
                    set(menu, cx, index, Some(part));
                    menu.on_take(cx, index, &target);
                    if !add_to_inventory(cx, &mut target) {
                        cx.throw(target, true);
                    }
                } else {
                    cx.set_player(button, Some(target.clone()));
                    set(menu, cx, index, Some(source));
                    menu.on_take(cx, index, &target);
                }
            }
        }
    }
}

/// `THROW`: drop one (button 0) or the stack (any other button). Button 1
/// repeats while the slot refills with the same item.
fn throw<M: Menu + ?Sized>(menu: &mut M, cx: &mut MenuContext, index: usize, button: i32) {
    let amount = if button == 0 {
        1
    } else {
        item(menu, cx, index).map_or(0, |stack| i32::from(stack.count))
    };
    let mut taken = safe_take(menu, cx, index, amount, i32::MAX);
    if let Some(stack) = &taken {
        cx.throw(stack.clone(), true);
    }
    if button != 1 {
        return;
    }
    while let Some(previous) = taken {
        if !item(menu, cx, index).is_some_and(|stack| stack.id == previous.id) {
            break;
        }
        taken = safe_take(menu, cx, index, amount, i32::MAX);
        if let Some(stack) = &taken {
            cx.throw(stack.clone(), true);
        }
    }
}

/// `PICKUP_ALL` (a double click): gather matching stacks onto the carried
/// one, from the first slot up (button 0) or the last down, partial stacks
/// in a first pass and full ones in a second, until the carried stack is
/// full. Only when the clicked slot is empty or can't be picked up.
fn pickup_all<M: Menu + ?Sized>(menu: &mut M, cx: &mut MenuContext, index: usize, button: i32) {
    let Some(mut carried) = cx.inventory.cursor.clone() else {
        return;
    };
    if item(menu, cx, index).is_some() && menu.may_pickup(cx, index) {
        return;
    }
    let len = menu.slots().len();
    for pass in 0..2 {
        for step in 0..len {
            if carried.count >= carried.max {
                break;
            }
            let target = if button == 0 { step } else { len - 1 - step };
            let Some(stack) = item(menu, cx, target) else {
                continue;
            };
            let count = i32::from(stack.count);
            let full = stack.count == stack.max;
            if !can_item_quick_replace(Some(stack), &carried, true)
                || !menu.may_pickup(cx, target)
                || !menu.can_take_for_pick_all(&carried, target)
                || (pass == 0 && full)
            {
                continue;
            }
            let room = i32::from(carried.max) - i32::from(carried.count);
            if let Some(taken) = safe_take(menu, cx, target, count, room) {
                carried.count += taken.count;
                cx.inventory.cursor = Some(carried.clone());
            }
        }
    }
}

/// `QUICK_CRAFT`, start to end, with the slots the mouse dragged over.
pub fn drag<M: Menu + ?Sized>(menu: &mut M, cx: &mut MenuContext, kind: i32, slots: &[usize]) {
    // The start: something must be carried, and the type valid
    // (`isValidQuickcraftType`: full stacks only in creative).
    let Some(carried) = cx.inventory.cursor.clone() else {
        return;
    };
    if !(kind == 0 || kind == 1 || (kind == 2 && cx.creative)) {
        return;
    }
    // Each add: the slot joins the set if it can take the carried item and,
    // unless placing full stacks, there are more items than slots so far.
    let len = menu.slots().len();
    let mut chosen: Vec<usize> = Vec::new();
    for &index in slots {
        if index < len
            && !chosen.contains(&index)
            && quick_craft_accepts(menu, cx, index, &carried, kind, chosen.len(), false)
        {
            chosen.push(index);
        }
    }
    // The end. One slot is a plain click, with the drag type as the button
    // (so a full-stack drag on one slot does nothing).
    match chosen.len() {
        0 => {}
        1 => click(menu, cx, chosen[0] as i32, kind, ContainerInput::Pickup),
        size => {
            // Every slot is given the same share of the stack the drag began
            // with, up to what the slot holds; the rest stays carried.
            let place = quick_craft_place_count(size, kind, &carried);
            let mut remaining = i32::from(carried.count);
            for &index in &chosen {
                if !quick_craft_accepts(menu, cx, index, &carried, kind, size, true) {
                    continue;
                }
                let existing = item(menu, cx, index).map_or(0, |stack| i32::from(stack.count));
                let max = i32::from(carried.max).min(menu.max_stack(cx, index, &carried));
                let count = (place + existing).min(max);
                remaining -= count - existing;
                let stack = (count > 0).then(|| ItemStack {
                    count: count as u8,
                    ..carried.clone()
                });
                set(menu, cx, index, stack);
            }
            cx.inventory.cursor = (remaining > 0).then_some(ItemStack {
                count: remaining as u8,
                ..carried
            });
        }
    }
}

/// Whether a drag of `carried` may cover `index`, with `size` slots chosen
/// so far (`at_end`: the end's re-check, which allows as many slots as
/// items).
fn quick_craft_accepts<M: Menu + ?Sized>(
    menu: &M,
    cx: &MenuContext,
    index: usize,
    carried: &ItemStack,
    kind: i32,
    size: usize,
    at_end: bool,
) -> bool {
    let count = usize::from(carried.count);
    can_item_quick_replace(item(menu, cx, index), carried, true)
        && menu.may_place(cx, index, carried)
        && (kind == 2 || if at_end { count >= size } else { count > size })
        && menu.can_drag_to(index)
}

/// `AbstractContainerMenu.getQuickCraftPlaceCount`: what a drag puts in each
/// of `size` slots (the screen's preview shows the same). The share is
/// floored float division, as vanilla's, so no `size` divides by zero.
pub fn quick_craft_place_count(size: usize, kind: i32, stack: &ItemStack) -> i32 {
    match kind {
        0 => (f32::from(stack.count) / size as f32).floor() as i32,
        1 => 1,
        2 => i32::from(stack.max),
        _ => i32::from(stack.count),
    }
}

/// `AbstractContainerMenu.canItemQuickReplace`: whether `stack` can go where
/// `slot` is: an empty slot, or the same item and components with room for
/// it (`ignore_size`: room for any of it, a full stack included).
pub fn can_item_quick_replace(
    slot: Option<&ItemStack>,
    stack: &ItemStack,
    ignore_size: bool,
) -> bool {
    match slot {
        None => true,
        Some(existing) if existing.same_item(stack) => {
            let incoming = if ignore_size {
                0
            } else {
                i32::from(stack.count)
            };
            i32::from(existing.count) + incoming <= i32::from(stack.max)
        }
        Some(_) => false,
    }
}

/// `ItemStack.isStackable`. Vanilla also rules out a damaged damageable
/// item, but a damageable item always stacks to 1.
pub fn is_stackable(stack: &ItemStack) -> bool {
    stack.max > 1
}

/// `AbstractContainerMenu.moveItemStackTo`: move `stack` into the slots
/// `start..end`, last first if `backwards`. A stackable item first tops up
/// the matching stacks there (without asking `mayPlace`); what is left goes
/// into the first empty slot that takes it, and only one: the rest stays in
/// `stack`. Whether anything moved. `stack` is detached; the caller writes
/// it back to the slot it came from.
pub fn move_item_stack_to<M: Menu + ?Sized>(
    menu: &mut M,
    cx: &mut MenuContext,
    stack: &mut ItemStack,
    start: usize,
    end: usize,
    backwards: bool,
) -> bool {
    let end = end.min(menu.slots().len());
    let order: Vec<usize> = if backwards {
        (start..end).rev().collect()
    } else {
        (start..end).collect()
    };
    let mut moved = false;
    if is_stackable(stack) {
        for &index in &order {
            if stack.count == 0 {
                break;
            }
            let Some(target) = item(menu, cx, index) else {
                continue;
            };
            if !target.same_item(stack) {
                continue;
            }
            let mut target = target.clone();
            let total = i32::from(target.count) + i32::from(stack.count);
            let max = menu.max_stack(cx, index, &target);
            if total <= max {
                stack.count = 0;
                target.count = total as u8;
            } else if i32::from(target.count) < max {
                stack.count -= (max - i32::from(target.count)) as u8;
                target.count = max as u8;
            } else {
                continue;
            }
            write(menu, cx, index, Some(target));
            set_changed(menu, cx, index);
            moved = true;
        }
    }
    if stack.count > 0 {
        for &index in &order {
            if item(menu, cx, index).is_none() && menu.may_place(cx, index, stack) {
                let max = menu.max_stack(cx, index, stack);
                let part = split(stack, i32::from(stack.count).min(max));
                set(menu, cx, index, Some(part));
                set_changed(menu, cx, index);
                moved = true;
                break;
            }
        }
    }
    moved
}

/// The storage menus' `quickMoveStack` (`ChestMenu`, `ShulkerBoxMenu`,
/// `HopperMenu`): from the container's `size` slots into the player's, last
/// first; from the player's into the container, first first.
pub fn container_quick_move<M: Menu + ?Sized>(
    menu: &mut M,
    cx: &mut MenuContext,
    index: usize,
    size: usize,
) -> Option<ItemStack> {
    let mut stack = item(menu, cx, index)?.clone();
    let clicked = stack.clone();
    let len = menu.slots().len();
    let moved = if index < size {
        move_item_stack_to(menu, cx, &mut stack, size, len, true)
    } else {
        move_item_stack_to(menu, cx, &mut stack, 0, size, false)
    };
    if !moved {
        return None;
    }
    put_back(menu, cx, index, stack);
    Some(clicked)
}

/// The end of a `quickMoveStack`: an emptied slot is cleared
/// (`setByPlayer(EMPTY)`), otherwise it keeps what was left (`setChanged`).
pub fn put_back<M: Menu + ?Sized>(
    menu: &mut M,
    cx: &mut MenuContext,
    index: usize,
    stack: ItemStack,
) {
    if stack.count == 0 {
        set(menu, cx, index, None);
    } else {
        write(menu, cx, index, Some(stack));
        set_changed(menu, cx, index);
    }
}

/// `Slot.getItem`.
pub fn item<'a, M: Menu + ?Sized>(
    menu: &'a M,
    cx: &'a MenuContext,
    index: usize,
) -> Option<&'a ItemStack> {
    match menu.slots().get(index)?.at {
        SlotRef::Own(own) => menu.own().get(own),
        SlotRef::Player(player) => cx.inventory.slots.get(player).and_then(Option::as_ref),
    }
}

/// Write a slot's stack, as its container's `setItem` does, without the
/// `setChanged` that follows.
pub fn write<M: Menu + ?Sized>(
    menu: &mut M,
    cx: &mut MenuContext,
    index: usize,
    stack: Option<ItemStack>,
) {
    let Some(slot) = menu.slots().get(index) else {
        return;
    };
    match slot.at {
        SlotRef::Own(own) => menu.own_mut().set(own, stack),
        SlotRef::Player(player) => cx.set_player(player, stack),
    }
}

/// `Slot.set` (and `setByPlayer`): write the slot and mark it changed.
pub fn set<M: Menu + ?Sized>(
    menu: &mut M,
    cx: &mut MenuContext,
    index: usize,
    stack: Option<ItemStack>,
) {
    write(menu, cx, index, stack);
    set_changed(menu, cx, index);
}

/// `Slot.setChanged`: an own slot's container tells the menu
/// ([`Menu::slots_changed`]); the player's inventory tells no one.
pub fn set_changed<M: Menu + ?Sized>(menu: &mut M, cx: &mut MenuContext, index: usize) {
    if let Some(SlotRef::Own(_)) = menu.slots().get(index).map(|slot| slot.at) {
        menu.slots_changed(cx, index);
    }
}

/// `Container.removeItem` for a plain container (`ContainerHelper.removeItem`):
/// split up to `amount` off the slot's stack.
pub fn remove_from<M: Menu + ?Sized>(
    menu: &mut M,
    cx: &mut MenuContext,
    index: usize,
    amount: i32,
) -> Option<ItemStack> {
    if amount <= 0 {
        return None;
    }
    let mut stack = item(menu, cx, index)?.clone();
    let taken = split(&mut stack, amount);
    write(menu, cx, index, (stack.count > 0).then_some(stack));
    set_changed(menu, cx, index);
    Some(taken)
}

/// `Slot.allowModification`: whether the player may both take from the slot
/// and put back what it holds.
pub fn allow_modification<M: Menu + ?Sized>(menu: &M, cx: &MenuContext, index: usize) -> bool {
    menu.may_pickup(cx, index)
        && item(menu, cx, index).is_none_or(|stack| menu.may_place(cx, index, stack))
}

/// `Slot.tryRemove`: take up to `amount`, but no more than `max`. A slot the
/// player may not modify (a result) gives all of its stack or nothing.
pub fn try_remove<M: Menu + ?Sized>(
    menu: &mut M,
    cx: &mut MenuContext,
    index: usize,
    amount: i32,
    max: i32,
) -> Option<ItemStack> {
    if !menu.may_pickup(cx, index) {
        return None;
    }
    let count = item(menu, cx, index).map_or(0, |stack| i32::from(stack.count));
    if max < count && !allow_modification(menu, cx, index) {
        return None;
    }
    let taken = menu.remove(cx, index, amount.min(max))?;
    if item(menu, cx, index).is_none() {
        set(menu, cx, index, None);
    }
    Some(taken)
}

/// `Slot.safeTake`: `tryRemove`, then `onTake` for what was taken.
pub fn safe_take<M: Menu + ?Sized>(
    menu: &mut M,
    cx: &mut MenuContext,
    index: usize,
    amount: i32,
    max: i32,
) -> Option<ItemStack> {
    let taken = try_remove(menu, cx, index, amount, max)?;
    menu.on_take(cx, index, &taken);
    Some(taken)
}

/// `Slot.safeInsert`: put up to `amount` of `stack` into the slot, if it
/// takes the item and is empty or holds the same; returns the rest.
pub fn safe_insert<M: Menu + ?Sized>(
    menu: &mut M,
    cx: &mut MenuContext,
    index: usize,
    mut stack: ItemStack,
    amount: i32,
) -> Option<ItemStack> {
    if stack.count == 0 {
        return None;
    }
    if !menu.may_place(cx, index, &stack) {
        return Some(stack);
    }
    let current = item(menu, cx, index).cloned();
    let room = menu.max_stack(cx, index, &stack)
        - current
            .as_ref()
            .map_or(0, |current| i32::from(current.count));
    let moved = amount.min(i32::from(stack.count)).min(room);
    if moved <= 0 {
        return Some(stack);
    }
    match current {
        None => {
            let part = split(&mut stack, moved);
            set(menu, cx, index, Some(part));
        }
        Some(mut current) if current.same_item(&stack) => {
            stack.count -= moved as u8;
            current.count += moved as u8;
            set(menu, cx, index, Some(current));
        }
        Some(_) => {}
    }
    (stack.count > 0).then_some(stack)
}

/// `ItemStack.split`: take up to `amount` off `stack`.
fn split(stack: &mut ItemStack, amount: i32) -> ItemStack {
    let amount = amount.clamp(0, i32::from(stack.count)) as u8;
    stack.count -= amount;
    ItemStack {
        count: amount,
        ..stack.clone()
    }
}

/// `AbstractContainerMenu.removed`'s base: the carried stack goes back into
/// the inventory (`placeItemBackInInventory`).
pub fn return_carried(cx: &mut MenuContext) {
    if let Some(carried) = cx.inventory.cursor.take() {
        place_item_back(cx, carried);
    }
}

/// `AbstractContainerMenu.clearContainer` over own slots: each stack goes
/// back into the inventory, as workstations return their inputs on close.
pub fn clear_own_slots<M: Menu + ?Sized>(
    menu: &mut M,
    cx: &mut MenuContext,
    own: std::ops::Range<usize>,
) {
    for index in own {
        // `removeItemNoUpdate`: no `setChanged`.
        let Some(stack) = menu.own().get(index).cloned() else {
            continue;
        };
        menu.own_mut().set(index, None);
        place_item_back(cx, stack);
    }
}

/// `Inventory.placeItemBackInInventory`: top up the stacks with room
/// (`getSlotWithRemainingSpace`: the selected slot, the offhand, then 0-35),
/// then the free slots, and drop what is left (not from the hand).
pub fn place_item_back(cx: &mut MenuContext, mut stack: ItemStack) {
    while stack.count > 0 {
        let Some(slot) = slot_with_remaining_space(cx, &stack).or_else(|| free_slot(cx)) else {
            cx.throw(stack, false);
            return;
        };
        let held = cx.inventory.slots[slot]
            .as_ref()
            .map_or(0, |held| i32::from(held.count));
        let room = i32::from(stack.max) - held;
        if room <= 0 {
            cx.throw(stack, false);
            return;
        }
        let mut part = split(&mut stack, room);
        add_resource_at(cx, slot, &mut part);
    }
}

/// `Inventory.add`: put `stack` into the inventory, taking what fits. True
/// when the last attempt placed something; in survival that means all of
/// it fitted, and otherwise the rest is left in `stack`. In creative what
/// does not fit is destroyed.
pub fn add_to_inventory(cx: &mut MenuContext, stack: &mut ItemStack) -> bool {
    if stack.count == 0 {
        return false;
    }
    let damaged = cx
        .inventory
        .recipes
        .durability(stack)
        .is_some_and(|(damage, _)| damage > 0);
    if damaged {
        if let Some(slot) = free_slot(cx) {
            cx.set_player(slot, Some(stack.clone()));
            stack.count = 0;
            return true;
        }
        if cx.creative {
            stack.count = 0;
            return true;
        }
        return false;
    }
    let mut last;
    loop {
        last = stack.count;
        if let Some(slot) = slot_with_remaining_space(cx, stack).or_else(|| free_slot(cx)) {
            add_resource_at(cx, slot, stack);
        }
        if stack.count == 0 || stack.count >= last {
            break;
        }
    }
    if stack.count == last && cx.creative {
        stack.count = 0;
        return true;
    }
    stack.count < last
}

/// `Inventory.addResource(slot, stack)`: move what fits of `stack` into
/// inventory slot `slot` (empty, or holding the same item).
fn add_resource_at(cx: &mut MenuContext, slot: usize, stack: &mut ItemStack) {
    let mut held = cx.inventory.slots[slot]
        .clone()
        .unwrap_or_else(|| ItemStack {
            count: 0,
            ..stack.clone()
        });
    let room = CONTAINER_MAX_STACK.min(i32::from(held.max)) - i32::from(held.count);
    let moved = i32::from(stack.count).min(room);
    if moved <= 0 {
        return;
    }
    held.count += moved as u8;
    stack.count -= moved as u8;
    cx.set_player(slot, Some(held));
}

/// `Inventory.getSlotWithRemainingSpace`: the first of the selected slot,
/// the offhand and slots 0-35 with a stack `stack` can top up.
fn slot_with_remaining_space(cx: &MenuContext, stack: &ItemStack) -> Option<usize> {
    let has_room = |index: usize| {
        cx.inventory
            .slots
            .get(index)
            .and_then(Option::as_ref)
            .is_some_and(|held| {
                held.same_item(stack)
                    && is_stackable(held)
                    && i32::from(held.count) < CONTAINER_MAX_STACK.min(i32::from(held.max))
            })
    };
    let selected = (cx.selected < 9).then_some(cx.selected);
    selected
        .into_iter()
        .chain([OFFHAND as usize])
        .chain(0..36)
        .find(|&index| has_room(index))
}

/// `Inventory.getFreeSlot`: the first empty slot of 0-35.
fn free_slot(cx: &MenuContext) -> Option<usize> {
    (0..36).find(|&index| cx.inventory.slots.get(index).is_some_and(Option::is_none))
}
