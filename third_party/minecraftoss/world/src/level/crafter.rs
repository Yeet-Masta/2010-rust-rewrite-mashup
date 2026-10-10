//! Crafters on the server level (26.3 `CrafterBlock` and
//! `CrafterBlockEntity`): the rising edge that schedules a craft, the craft
//! with its result and remainders dispensed from the front (into the
//! container there, or thrown), the `crafting` state's flash, the slots'
//! states, the hopper rule that fills the grid evenly, and the comparator
//! signal.
//!
//! What the grid crafts comes from the server's recipe manager, through
//! [`Crafting`]. A level without one crafts nothing: every craft fails.
//! Not simulated: `onCraftedBySystem` and the `crafter_recipe_crafted`
//! trigger.

use super::container::{ContainerRef, Stack, Store};
use super::{update, Level};
use minecraftoss_core::nbt::Tag;
use minecraftoss_core::pos::Direction;
use minecraftoss_core::{BlockPos, BlockStateId};
use std::collections::BTreeMap;

/// `CrafterBlock.MAX_CRAFTING_TICKS`: how long the `crafting` state shows.
const MAX_CRAFTING_TICKS: i32 = 6;
/// `CrafterBlock.CRAFTING_TICK_DELAY`.
const CRAFTING_TICK_DELAY: i32 = 4;
/// `LevelEvent.SOUND_CRAFTER_CRAFT`, `SOUND_CRAFTER_FAIL` and
/// `PARTICLES_SHOOT_WHITE_SMOKE`.
pub const CRAFTER_CRAFT: i32 = 1049;
pub const CRAFTER_FAIL: i32 = 1050;
pub const SHOOT_WHITE_SMOKE: i32 = 2010;

/// The recipe manager as crafters read it.
pub trait Crafting: Send + Sync {
    /// `CrafterBlock.getPotentialResults`, `assemble` and
    /// `getRemainingItems` for a 3 by 3 grid: the result of the recipe it
    /// matches, and what each grid slot leaves (its item's crafting
    /// remainder), in order, empty ones left out.
    fn craft(&self, grid: &[Stack]) -> Option<(Stack, Vec<Stack>)>;
}

/// A crafter block entity's state, as it saves it.
#[derive(Clone, Debug, PartialEq)]
pub struct Crafter {
    pub items: [Stack; 9],
    pub crafting_ticks_remaining: i32,
    /// `containerData`'s slot states: which slots are disabled.
    pub disabled: [bool; 9],
    /// `containerData`'s `triggered`.
    pub triggered: bool,
}

impl Default for Crafter {
    fn default() -> Self {
        Self { items: std::array::from_fn(|_| Stack::empty()), crafting_ticks_remaining: 0, disabled: [false; 9], triggered: false }
    }
}

impl Crafter {
    /// `loadAdditional`: a slot is disabled only if it is empty
    /// (`slotCanBeDisabled`).
    pub fn from_tag(tag: &Tag) -> Self {
        let int = |key: &str| tag.get(key).and_then(Tag::as_i64).unwrap_or(0) as i32;
        let mut crafter = Self { crafting_ticks_remaining: int("crafting_ticks_remaining"), triggered: int("triggered") == 1, ..Self::default() };
        for (slot, stack) in tag.get("Items").and_then(Tag::as_list).into_iter().flatten().filter_map(Stack::from_tag) {
            if let Some(item) = crafter.items.get_mut(slot) {
                *item = stack;
            }
        }
        let disabled = tag.get("disabled_slots").and_then(Tag::as_ints).into_iter().flatten();
        for slot in disabled.filter_map(|slot| usize::try_from(slot).ok()).filter(|&slot| slot < 9) {
            if crafter.items[slot].is_empty() {
                crafter.disabled[slot] = true;
            }
        }
        crafter
    }

    /// `saveAdditional`, into the block entity's tag. A crafter that still
    /// has its loot table saves that instead of its items
    /// (`trySaveLootTable`).
    pub fn save(&self, map: &mut BTreeMap<String, Tag>) {
        map.insert("crafting_ticks_remaining".to_owned(), Tag::Int(self.crafting_ticks_remaining));
        if !map.contains_key("LootTable") {
            let items = self.items.iter().enumerate().filter(|(_, s)| !s.is_empty()).map(|(i, s)| s.to_tag(i)).collect();
            map.insert("Items".to_owned(), Tag::List(items));
        }
        let disabled = (0..9).filter(|&slot| self.disabled[slot]).map(|slot| slot as i32).collect();
        map.insert("disabled_slots".to_owned(), Tag::IntArray(disabled));
        map.insert("triggered".to_owned(), Tag::Int(i32::from(self.triggered)));
    }

    /// `containerData`: each slot's state (1 disabled), then `triggered`.
    pub fn data(&self) -> [i32; 10] {
        let mut data = [0; 10];
        for (value, disabled) in data.iter_mut().zip(self.disabled) {
            *value = i32::from(disabled);
        }
        data[9] = i32::from(self.triggered);
        data
    }

    /// `setSlotState`: only an empty slot takes a state. Whether it did.
    pub fn set_slot_state(&mut self, slot: usize, enabled: bool) -> bool {
        if slot >= 9 || !self.items[slot].is_empty() {
            return false;
        }
        self.disabled[slot] = !enabled;
        true
    }

    /// `canPlaceItem`: a hopper puts nothing in a disabled or full slot,
    /// nor in one that holds some while a later enabled slot is empty or
    /// holds fewer of the same (`smallerStackExist`), so the grid fills
    /// evenly. `max_stack` is the item's maximum stack size.
    pub fn can_place(&self, slot: usize, max_stack: impl Fn(&Stack) -> i32) -> bool {
        let Some(current) = self.items.get(slot) else { return false };
        if self.disabled[slot] {
            return false;
        }
        if current.is_empty() {
            return true;
        }
        if current.count >= max_stack(current) {
            return false;
        }
        let smaller = (slot + 1..9).filter(|&i| !self.disabled[i]).any(|i| {
            let other = &self.items[i];
            other.is_empty() || other.count < current.count && other.same_item_same_components(current)
        });
        !smaller
    }

    /// `getRedstoneSignal`: the slots that hold something or are disabled.
    pub fn signal(&self) -> i32 {
        (0..9).filter(|&slot| !self.items[slot].is_empty() || self.disabled[slot]).count() as i32
    }
}

impl Level<'_> {
    /// The crafter block entity at a position.
    pub fn crafter(&self, pos: BlockPos) -> Option<Crafter> {
        if !self.is_a(self.block(pos), "CrafterBlock") {
            return None;
        }
        self.block_entity(pos).map(Crafter::from_tag)
    }

    /// Writes the crafter's state into its block entity, when it changed.
    fn put_crafter(&mut self, pos: BlockPos, crafter: &Crafter) {
        if self.crafter(pos).as_ref() == Some(crafter) {
            return;
        }
        if let Some(Tag::Compound(map)) = self.block_entity_mut(pos) {
            crafter.save(map);
        }
    }

    /// The crafter's `containerData` for its menu.
    pub fn crafter_data(&self, pos: BlockPos) -> Option<[i32; 10]> {
        self.crafter(pos).map(|crafter| crafter.data())
    }

    /// What the crafter's grid crafts now (`CrafterMenu.refreshRecipeResult`).
    pub fn crafter_result(&self, pos: BlockPos) -> Option<Stack> {
        let crafter = self.crafter(pos)?;
        let (result, _) = self.crafting.as_ref()?.craft(&crafter.items)?;
        (!result.is_empty()).then_some(result)
    }

    /// `CrafterBlockEntity.setSlotState`, from the player's menu: an empty
    /// slot takes the state, and comparators read the crafter again.
    pub fn crafter_set_slot_state(&mut self, pos: BlockPos, slot: usize, enabled: bool) {
        let Some(mut crafter) = self.crafter(pos) else { return };
        if crafter.set_slot_state(slot, enabled) {
            self.put_crafter(pos, &crafter);
            self.block_entity_changed(pos);
        }
    }

    /// `CrafterBlockEntity.setItem`'s rule, before the stack goes in: a
    /// disabled slot given a stack is enabled again.
    pub(super) fn crafter_set_item(&mut self, pos: BlockPos, slot: usize) {
        let Some(mut crafter) = self.crafter(pos) else { return };
        if crafter.disabled.get(slot) == Some(&true) && crafter.set_slot_state(slot, true) {
            self.put_crafter(pos, &crafter);
        }
    }

    /// `canPlaceItem` for a crafter's slot.
    pub(super) fn crafter_can_place(&self, pos: BlockPos, slot: usize) -> bool {
        self.crafter(pos).is_some_and(|crafter| crafter.can_place(slot, |stack| self.item_max_stack(stack)))
    }

    /// `CrafterBlock.getAnalogOutputSignal`.
    pub(super) fn crafter_signal(&self, pos: BlockPos) -> i32 {
        self.crafter(pos).map_or(0, |crafter| crafter.signal())
    }

    /// `CrafterBlock.newBlockEntity`: a new block entity is triggered as its
    /// block is.
    pub(super) fn crafter_created(&mut self, pos: BlockPos, state: BlockStateId) {
        if !self.is_a(state, "CrafterBlock") || self.registries().blocks.property(state, "triggered") != Some("true") {
            return;
        }
        if let Some(mut crafter) = self.crafter(pos) {
            crafter.triggered = true;
            self.put_crafter(pos, &crafter);
        }
    }

    /// `CrafterBlock.getStateForPlacement`'s `triggered`: whether the block
    /// is powered where it goes. Another block's state is left as it is.
    pub fn crafter_placement_state(&self, pos: BlockPos, state: BlockStateId) -> BlockStateId {
        if !self.is_a(state, "CrafterBlock") {
            return state;
        }
        self.with(state, "triggered", if self.has_neighbor_signal(pos) { "true" } else { "false" })
    }

    /// `CrafterBlock.setPlacedBy`: a crafter placed triggered crafts in 4
    /// ticks.
    pub fn crafter_placed(&mut self, pos: BlockPos) {
        let state = self.block(pos);
        if self.is_a(state, "CrafterBlock") && self.registries().blocks.property(state, "triggered") == Some("true") {
            let block = self.block_id(state);
            self.schedule_block_tick_priority(pos, block, CRAFTING_TICK_DELAY, super::redstone::priority::NORMAL);
        }
    }

    /// `CrafterBlock.neighborChanged`: a rising edge schedules a craft and
    /// triggers; a falling one ends `triggered` and `crafting`.
    pub(super) fn crafter_neighbor_changed(&mut self, state: BlockStateId, pos: BlockPos) {
        let should = self.has_neighbor_signal(pos);
        let triggered = self.registries().blocks.property(state, "triggered") == Some("true");
        if should && !triggered {
            let block = self.block_id(state);
            self.schedule_block_tick_priority(pos, block, CRAFTING_TICK_DELAY, super::redstone::priority::NORMAL);
            self.set_block(pos, self.with(state, "triggered", "true"), update::CLIENTS, update::LIMIT);
            self.crafter_set_triggered(pos, true);
        } else if !should && triggered {
            let off = self.with(self.with(state, "triggered", "false"), "crafting", "false");
            self.set_block(pos, off, update::CLIENTS, update::LIMIT);
            self.crafter_set_triggered(pos, false);
        }
    }

    /// `CrafterBlockEntity.setTriggered`.
    fn crafter_set_triggered(&mut self, pos: BlockPos, triggered: bool) {
        if let Some(mut crafter) = self.crafter(pos) {
            crafter.triggered = triggered;
            self.put_crafter(pos, &crafter);
        }
    }

    /// The way the crafter faces (`ORIENTATION.front()`).
    fn crafter_front(&self, state: BlockStateId) -> Direction {
        let orientation = self.registries().blocks.property(state, "orientation").unwrap_or("north_up");
        orientation.split('_').next().and_then(Direction::from_name).unwrap_or(Direction::North)
    }

    /// `CrafterBlock.dispenseFrom` (its scheduled tick): the grid crafts,
    /// or the crafter fails with its sound. A craft shows `crafting`,
    /// dispenses the result then each remainder, and takes one of every
    /// stack in the grid.
    pub(super) fn crafter_dispense(&mut self, state: BlockStateId, pos: BlockPos) {
        let Some(mut crafter) = self.crafter(pos) else { return };
        let crafted = self.crafting.as_ref().and_then(|crafting| crafting.craft(&crafter.items));
        let Some((result, remainders)) = crafted.filter(|(result, _)| !result.is_empty()) else {
            self.level_event(CRAFTER_FAIL, pos, 0);
            return;
        };
        crafter.crafting_ticks_remaining = MAX_CRAFTING_TICKS;
        self.put_crafter(pos, &crafter);
        self.set_block(pos, self.with(state, "crafting", "true"), update::CLIENTS, update::LIMIT);
        self.crafter_dispense_item(pos, state, result);
        for remainder in remainders.into_iter().filter(|stack| !stack.is_empty()) {
            self.crafter_dispense_item(pos, state, remainder);
        }
        let Some(mut crafter) = self.crafter(pos) else { return };
        for stack in crafter.items.iter_mut().filter(|stack| !stack.is_empty()) {
            stack.count -= 1;
            if stack.is_empty() {
                *stack = Stack::empty();
            }
        }
        self.put_crafter(pos, &crafter);
        self.block_entity_changed(pos);
    }

    /// `CrafterBlock.dispenseItem`: into the container in front, one at a
    /// time when that is a crafter or the stack is more than it holds in a
    /// slot, else as much at a time as goes; what is left is thrown from the
    /// front (`DefaultDispenseItemBehavior.spawnItem`, accuracy 6) with the
    /// craft's sound and smoke.
    fn crafter_dispense_item(&mut self, pos: BlockPos, state: BlockStateId, results: Stack) {
        let direction = self.crafter_front(state);
        let me = ContainerRef::Single(pos, Store::Crafter);
        let mut remaining = results;
        if let Some(into) = self.container_at(pos.relative(direction, 1), true) {
            let one_at_a_time = matches!(into, ContainerRef::Single(_, Store::Crafter)) || remaining.count > self.container_max_stack(&remaining);
            while !remaining.is_empty() {
                let before = remaining.count;
                if one_at_a_time {
                    let mut one = remaining.clone();
                    one.count = 1;
                    if !self.add_item(Some(me), into, one, Some(direction.opposite())).is_empty() {
                        break;
                    }
                    remaining.count -= 1;
                } else {
                    remaining = self.add_item(Some(me), into, remaining, Some(direction.opposite()));
                    if remaining.count == before {
                        break;
                    }
                }
            }
        }
        if !remaining.is_empty() {
            self.spawn_item(pos, direction, remaining);
            self.level_event(CRAFTER_CRAFT, pos, 0);
            self.level_event(SHOOT_WHITE_SMOKE, pos, direction.index() as i32);
        }
    }

    /// `CrafterBlockEntity.serverTick`: the `crafting` state ends when the
    /// crafting ticks run out.
    pub(super) fn crafter_tick(&mut self, pos: BlockPos) {
        let Some(mut crafter) = self.crafter(pos) else { return };
        let remaining = crafter.crafting_ticks_remaining - 1;
        if remaining < 0 {
            return;
        }
        crafter.crafting_ticks_remaining = remaining;
        self.put_crafter(pos, &crafter);
        if remaining == 0 {
            let state = self.block(pos);
            self.set_block_and_update(pos, self.with(state, "crafting", "false"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(stacks: &[(usize, &str, i32)]) -> Crafter {
        let mut crafter = Crafter::default();
        for &(slot, id, count) in stacks {
            crafter.items[slot] = Stack::new(id, count);
        }
        crafter
    }

    #[test]
    fn hoppers_fill_the_grid_evenly() {
        let max = |_: &Stack| 64;
        let mut crafter = grid(&[(0, "minecraft:stick", 2), (1, "minecraft:stick", 1)]);
        crafter.disabled[4] = true;
        assert!(!crafter.can_place(0, max), "slot 1 holds fewer");
        assert!(!crafter.can_place(1, max), "slot 2 is empty");
        assert!(crafter.can_place(2, max));
        assert!(!crafter.can_place(4, max), "disabled");
        for slot in [2, 3, 5, 6, 7, 8] {
            crafter.items[slot] = Stack::new("minecraft:stick", 2);
        }
        assert!(crafter.can_place(1, max), "every later slot holds as many or more");
        assert!(!crafter.can_place(0, max), "slot 1 holds fewer still");
        crafter.items[8] = Stack::new("minecraft:dirt", 1);
        crafter.items[1].count = 2;
        assert!(crafter.can_place(0, max), "fewer of another item is no matter");
        crafter.items[0].count = 64;
        assert!(!crafter.can_place(0, max), "full");
    }

    #[test]
    fn the_signal_counts_filled_and_disabled_slots() {
        let mut crafter = grid(&[(0, "minecraft:stick", 2), (8, "minecraft:dirt", 1)]);
        assert_eq!(crafter.signal(), 2);
        assert!(crafter.set_slot_state(3, false));
        assert!(!crafter.set_slot_state(0, false), "slot 0 holds sticks");
        assert_eq!(crafter.signal(), 3);
        assert_eq!(crafter.data(), [0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn the_state_saves_as_the_block_entity_does() {
        let mut crafter = grid(&[(0, "minecraft:stick", 2)]);
        crafter.disabled[5] = true;
        crafter.triggered = true;
        crafter.crafting_ticks_remaining = 3;
        let mut map = BTreeMap::new();
        crafter.save(&mut map);
        assert_eq!(map.get("disabled_slots"), Some(&Tag::IntArray(vec![5])));
        assert_eq!(map.get("triggered"), Some(&Tag::Int(1)));
        assert_eq!(Crafter::from_tag(&Tag::Compound(map.clone())), crafter);
        // A slot saved disabled while it holds something loads enabled.
        map.insert("disabled_slots".to_owned(), Tag::IntArray(vec![0, 5, 12]));
        assert_eq!(Crafter::from_tag(&Tag::Compound(map)).disabled, crafter.disabled);
    }
}
