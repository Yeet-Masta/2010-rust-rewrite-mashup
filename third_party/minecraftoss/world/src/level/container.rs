//! Block containers and hoppers on the server level (26.3 `Container`,
//! `BaseContainerBlockEntity`, `CompoundContainer` for double chests,
//! `ShulkerBoxBlockEntity`'s faces, `HopperBlockEntity` and
//! `AbstractContainerMenu.getRedstoneSignalFromContainer`).
//!
//! Items live in the block entity's saved tag (`Items`), so every change is
//! what the chunk saves. Simulated containers: chests (single and double,
//! trapped and copper), barrels, shulker boxes, hoppers, dispensers and
//! droppers. Not yet: furnaces, brewing stands, crafters, bookshelves, pots,
//! shelves, container entities, item entities.
//!
//! A container generation left with a loot table is filled from it the
//! first time something takes from it, puts into it or opens it
//! (`RandomizableContainer.unpackLootTable`): a hopper, a dropper, a break,
//! a player. A comparator reading it does not unpack it yet: it reads it
//! empty.

use super::redstone::Kind;
use super::Level;
use minecraftoss_core::nbt::Tag;
use minecraftoss_core::pos::Direction;
use minecraftoss_core::{BlockPos, BlockStateId};
use std::collections::BTreeMap;

pub use minecraftoss_core::item::ItemStack as Stack;

/// Which block entity holds the items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    Chest,
    Barrel,
    ShulkerBox,
    Hopper,
    Dispenser,
}

impl Store {
    /// `getContainerSize`.
    pub fn size(self) -> usize {
        match self {
            Self::Hopper => 5,
            Self::Dispenser => 9,
            _ => 27,
        }
    }
}

/// A container a hopper or comparator reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerRef {
    Single(BlockPos, Store),
    /// `CompoundContainer(first, second)` of a double chest.
    Double(BlockPos, BlockPos),
}

impl ContainerRef {
    /// `getContainerSize`: 54 for a double chest.
    pub fn size(self) -> usize {
        match self {
            Self::Single(_, store) => store.size(),
            Self::Double(..) => 54,
        }
    }

    /// The block entity and slot within it.
    fn locate(self, slot: usize) -> (BlockPos, Store, usize) {
        match self {
            Self::Single(pos, store) => (pos, store, slot),
            Self::Double(first, second) => {
                if slot >= 27 {
                    (second, Store::Chest, slot - 27)
                } else {
                    (first, Store::Chest, slot)
                }
            }
        }
    }

    /// The block entities behind it: a double chest's first half, then its
    /// second.
    pub fn positions(self) -> Vec<BlockPos> {
        match self {
            Self::Single(pos, _) => vec![pos],
            Self::Double(first, second) => vec![first, second],
        }
    }
}

impl Level<'_> {
    /// The block entity that holds a block's items, for the simulated
    /// containers.
    pub fn store_of(&self, state: BlockStateId) -> Option<Store> {
        let blocks = &self.registries().blocks;
        let info = blocks.block(blocks.block_of(state));
        if info.is_a("ChestBlock") {
            Some(Store::Chest)
        } else if info.is_a("BarrelBlock") {
            Some(Store::Barrel)
        } else if info.is_a("ShulkerBoxBlock") {
            Some(Store::ShulkerBox)
        } else if info.is_a("HopperBlock") {
            Some(Store::Hopper)
        } else if info.is_a("DispenserBlock") {
            Some(Store::Dispenser)
        } else {
            None
        }
    }

    /// The saved tag of the block entity at a position.
    pub fn block_entity(&self, pos: BlockPos) -> Option<&Tag> {
        self.chunk(pos.chunk())?.block_entities.entities.get(&(pos.x, pos.y, pos.z))
    }

    /// A block entity to change: the change is noted for the chunk's save.
    pub(super) fn block_entity_mut(&mut self, pos: BlockPos) -> Option<&mut Tag> {
        let tag = self.chunks.get_mut(&pos.chunk())?.block_entities.entities.get_mut(&(pos.x, pos.y, pos.z))?;
        self.block_entities_changed.insert((pos.x, pos.y, pos.z));
        Some(tag)
    }

    /// `ChestBlock.isChestBlockedAt`: a redstone conductor above, or a cat
    /// sitting in the block's space above.
    fn chest_blocked(&self, pos: BlockPos) -> bool {
        let above = self.block(pos.above());
        if self.registries().blocks.is(above, minecraftoss_core::block::flags::REDSTONE_CONDUCTOR) {
            return true;
        }
        let (x, y, z) = (f64::from(pos.x), f64::from(pos.y), f64::from(pos.z));
        let space = super::physics::Aabb::new(x, y + 1.0, z, x + 1.0, y + 2.0, z + 1.0);
        self.sitting_cats.iter().any(|cat| cat.intersects(&space))
    }

    /// `HopperBlockEntity.getBlockContainer` / `ChestBlock.getContainer`.
    pub fn container_at(&self, pos: BlockPos, ignore_blocked: bool) -> Option<ContainerRef> {
        let state = self.block(pos);
        let store = self.store_of(state)?;
        self.block_entity(pos)?;
        if store != Store::Chest {
            return Some(ContainerRef::Single(pos, store));
        }
        if !ignore_blocked && self.chest_blocked(pos) {
            return None;
        }
        let blocks = &self.registries().blocks;
        let kind = blocks.property(state, "type").unwrap_or("single");
        if kind == "single" {
            return Some(ContainerRef::Single(pos, store));
        }
        let facing = blocks.property(state, "facing").and_then(Direction::from_name).unwrap_or(Direction::North);
        let connected = if kind == "left" { facing.clockwise() } else { facing.counter_clockwise() };
        let neighbour_pos = pos.relative(connected, 1);
        let neighbour = self.block(neighbour_pos);
        let same = blocks.block_of(neighbour) == blocks.block_of(state);
        let neighbour_kind = blocks.property(neighbour, "type").unwrap_or("single");
        if same && neighbour_kind != "single" && neighbour_kind != kind && blocks.property(neighbour, "facing") == blocks.property(state, "facing") {
            if !ignore_blocked && self.chest_blocked(neighbour_pos) {
                return None;
            }
            if self.block_entity(neighbour_pos).is_some() {
                // `RIGHT` is `FIRST`.
                let (first, second) = if kind == "right" { (pos, neighbour_pos) } else { (neighbour_pos, pos) };
                return Some(ContainerRef::Double(first, second));
            }
        }
        Some(ContainerRef::Single(pos, store))
    }

    fn read_items(&self, pos: BlockPos, size: usize) -> Vec<Stack> {
        let mut items = vec![Stack::empty(); size];
        if let Some(list) = self.block_entity(pos).and_then(|t| t.get("Items")).and_then(Tag::as_list) {
            for (slot, stack) in list.iter().filter_map(Stack::from_tag) {
                if slot < size {
                    items[slot] = stack;
                }
            }
        }
        items
    }

    fn write_items(&mut self, pos: BlockPos, items: &[Stack]) {
        let list: Vec<Tag> = items.iter().enumerate().filter(|(_, s)| !s.is_empty()).map(|(i, s)| s.to_tag(i)).collect();
        if let Some(Tag::Compound(map)) = self.block_entity_mut(pos) {
            map.insert("Items".to_owned(), Tag::List(list));
        }
    }

    /// `RandomizableContainer.unpackLootTable`: a container that still has
    /// the loot table generation gave it (`LootTable`, with its
    /// `LootTableSeed`) is filled from it (`LootTable.fill`) and keeps it no
    /// more. `opener_luck` is the opening player's luck (`withLuck`, the
    /// player being `THIS_ENTITY`); none when no player opens it. A table
    /// the loot engine cannot run is left as it is, and noted, rather than
    /// fill the container with anything else.
    pub fn unpack_loot_table(&mut self, pos: BlockPos, opener_luck: Option<f32>) {
        let Some(entity) = self.block_entity(pos) else { return };
        let Some(table) = entity.get("LootTable").and_then(Tag::as_str).map(str::to_owned) else { return };
        let seed = entity.get("LootTableSeed").and_then(Tag::as_i64).unwrap_or(0);
        let Some(store) = self.store_of(self.block(pos)) else { return };
        let mut items = self.read_items(pos, store.size());
        let empty: Vec<usize> = (0..items.len()).filter(|&slot| items[slot].is_empty()).collect();
        let params = minecraftoss_core::loot::LootParams {
            // `Vec3.atCenterOf(worldPosition)`.
            origin: Some([f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5]),
            this_entity: opener_luck.is_some(),
            luck: opener_luck.unwrap_or(0.0),
            biome: self.biome_id(pos),
            ..minecraftoss_core::loot::LootParams::default()
        };
        let registries = self.lib.registries.clone();
        let placed = match registries.loot.fill(&registries, &table, &params, seed, &mut self.random_sequences, &mut self.random, &empty) {
            Ok(placed) => placed,
            Err(e) => {
                self.unsupported.push(format!("loot table {table}: {e}"));
                return;
            }
        };
        if let Some(Tag::Compound(map)) = self.block_entity_mut(pos) {
            map.remove("LootTable");
            map.remove("LootTableSeed");
        }
        // `setItem` for each, capped at what the slot holds.
        for (slot, mut stack) in placed {
            stack.count = stack.count.min(self.container_max_stack(&stack));
            items[slot] = stack;
        }
        self.write_items(pos, &items);
        self.block_entity_changed(pos);
    }

    /// Each block entity of a container unpacks its loot table, with no
    /// player.
    pub(super) fn unpack_container(&mut self, c: ContainerRef) {
        for pos in c.positions() {
            self.unpack_loot_table(pos, None);
        }
    }

    /// The biome at a block (`Level.getBiome`, through the biome zoom), by
    /// id.
    fn biome_id(&self, pos: BlockPos) -> Option<String> {
        let zoom = minecraftoss_generator::zoom::zoom_seed(self.random_sequences.world_seed);
        let [qx, qy, qz] = minecraftoss_generator::zoom::quart_for_block(zoom, pos.x, pos.y, pos.z);
        let chunk = self.chunk(minecraftoss_core::ChunkPos::new(qx >> 2, qz >> 2))?;
        let biome = chunk.biome((qx & 3) as usize, qy, (qz & 3) as usize);
        Some(self.registries().biomes.get(biome).name.to_string())
    }

    /// `item replace block ... container.N with ...`: the block entity's own
    /// container, `setItem` through its slot access.
    pub fn replace_block_item(&mut self, pos: BlockPos, slot: usize, stack: Stack) -> bool {
        let Some(store) = self.store_of(self.block(pos)) else { return false };
        if self.block_entity(pos).is_none() || slot >= store.size() {
            return false;
        }
        self.container_set_item(ContainerRef::Single(pos, store), slot, stack);
        true
    }

    /// A block entity's own container slots (what the harness observes).
    pub fn block_container_items(&self, pos: BlockPos) -> Option<Vec<Stack>> {
        let store = self.store_of(self.block(pos))?;
        self.block_entity(pos)?;
        Some(self.read_items(pos, store.size()))
    }

    /// Every slot of a container, a double chest's first half first.
    pub fn container_items(&self, c: ContainerRef) -> Vec<Stack> {
        match c {
            ContainerRef::Single(pos, store) => self.read_items(pos, store.size()),
            ContainerRef::Double(first, second) => {
                let mut items = self.read_items(first, 27);
                items.extend(self.read_items(second, 27));
                items
            }
        }
    }

    pub fn container_item(&self, c: ContainerRef, slot: usize) -> Stack {
        let (pos, store, slot) = c.locate(slot);
        self.read_items(pos, store.size()).swap_remove(slot)
    }

    /// Writes a slot with no side effects.
    pub(super) fn put_item(&mut self, c: ContainerRef, slot: usize, stack: Stack) {
        let (pos, store, slot) = c.locate(slot);
        let mut items = self.read_items(pos, store.size());
        items[slot] = stack;
        self.write_items(pos, &items);
    }

    /// `ItemStack.getMaxStackSize`, with a `max_stack_size` component.
    pub fn item_max_stack(&self, stack: &Stack) -> i32 {
        stack
            .components
            .as_ref()
            .and_then(|c| c.get("minecraft:max_stack_size"))
            .and_then(Tag::as_i64)
            .map_or_else(|| self.registries().items.max_stack(&stack.id), |m| m as i32)
    }

    /// `Container.getMaxStackSize(itemStack)`: 99 for every simulated container.
    fn container_max_stack(&self, stack: &Stack) -> i32 {
        self.item_max_stack(stack).min(99)
    }

    /// `BlockEntity.setChanged`: comparators re-read the container.
    fn block_entity_changed(&mut self, pos: BlockPos) {
        let state = self.block(pos);
        if !self.registries().blocks.is_air(state) {
            let block = self.block_id(state);
            self.update_neighbour_for_output_signal(pos, block);
        }
    }

    pub fn container_set_changed(&mut self, c: ContainerRef) {
        for pos in c.positions() {
            self.block_entity_changed(pos);
        }
    }

    /// `Container.setItem`: hoppers do not report the change themselves.
    pub fn container_set_item(&mut self, c: ContainerRef, slot: usize, mut stack: Stack) {
        let max = self.container_max_stack(&stack);
        if !stack.is_empty() && stack.count > max {
            stack.count = max;
        }
        self.put_item(c, slot, stack);
        let (pos, store, _) = c.locate(slot);
        if store != Store::Hopper {
            self.block_entity_changed(pos);
        }
    }

    /// `Container.removeItem` (`ContainerHelper.removeItem`): splits off up
    /// to `count`; the slot keeps what is left, possibly an empty stack.
    fn container_remove_item(&mut self, c: ContainerRef, slot: usize, count: i32) -> Stack {
        let mut current = self.container_item(c, slot);
        if current.is_empty() || count <= 0 {
            return Stack::empty();
        }
        let taken = count.min(current.count);
        let mut result = current.clone();
        result.count = taken;
        current.count -= taken;
        self.put_item(c, slot, current);
        let (pos, store, _) = c.locate(slot);
        if store != Store::Hopper {
            self.block_entity_changed(pos);
        }
        result
    }

    fn container_is_empty(&self, c: ContainerRef) -> bool {
        (0..c.size()).all(|slot| self.container_item(c, slot).is_empty())
    }

    /// `WorldlyContainer.getSlotsForFace`, or every slot.
    fn container_slots(&self, c: ContainerRef, _direction: Direction) -> Vec<usize> {
        (0..c.size()).collect()
    }

    /// `canPlaceItem` and, for worldly containers, `canPlaceItemThroughFace`.
    fn container_can_place(&self, c: ContainerRef, _slot: usize, stack: &Stack, direction: Option<Direction>) -> bool {
        match c {
            ContainerRef::Single(_, Store::ShulkerBox) if direction.is_some() => !self.is_shulker_box_item(&stack.id),
            _ => true,
        }
    }

    fn is_shulker_box_item(&self, id: &str) -> bool {
        let blocks = &self.registries().blocks;
        blocks.block_by_name(id).is_some_and(|b| blocks.block(b).is_a("ShulkerBoxBlock"))
    }

    /// `AbstractContainerMenu.getRedstoneSignalFromContainer`.
    pub(super) fn container_signal(&self, c: Option<ContainerRef>) -> i32 {
        let Some(c) = c else { return 0 };
        let mut total = 0.0f32;
        for slot in 0..c.size() {
            let stack = self.container_item(c, slot);
            if !stack.is_empty() {
                total += stack.count as f32 / self.container_max_stack(&stack) as f32;
            }
        }
        total /= c.size() as f32;
        // `Mth.lerpDiscrete(total, 0, 15)`.
        (total * 14.0).floor() as i32 + i32::from(total > 0.0)
    }

    // ---- item components ----------------------------------------------------------

    /// `BlockEntity.collectComponents` for a container's block entity: the
    /// components it keeps, with `BaseContainerBlockEntity`'s and
    /// `RandomizableContainerBlockEntity`'s own (`custom_name`, `lock`,
    /// `container` and `container_loot`), as saved component NBT. Empty
    /// contents are left out: they are every container item's default, so
    /// a copy of them is no change.
    pub fn container_components(&self, pos: BlockPos) -> Option<Tag> {
        let store = self.store_of(self.block(pos))?;
        let entity = self.block_entity(pos)?;
        let mut components = match entity.get("components") {
            Some(Tag::Compound(kept)) => kept.clone(),
            _ => BTreeMap::new(),
        };
        if let Some(name) = entity.get("CustomName") {
            components.insert("minecraft:custom_name".to_owned(), name.clone());
        }
        if let Some(lock) = entity.get("lock") {
            components.insert("minecraft:lock".to_owned(), lock.clone());
        }
        // `ItemContainerContents.fromItems`, saved as its slots.
        let slots: Vec<Tag> = self.read_items(pos, store.size()).iter().enumerate().filter(|(_, stack)| !stack.is_empty()).map(|(slot, stack)| container_slot(slot, stack)).collect();
        if !slots.is_empty() {
            components.insert("minecraft:container".to_owned(), Tag::List(slots));
        }
        if let Some(table) = entity.get("LootTable") {
            let mut loot = BTreeMap::from([("loot_table".to_owned(), table.clone())]);
            if let Some(seed) = entity.get("LootTableSeed").and_then(Tag::as_i64).filter(|&seed| seed != 0) {
                loot.insert("seed".to_owned(), Tag::Long(seed));
            }
            components.insert("minecraft:container_loot".to_owned(), Tag::Compound(loot));
        }
        Some(Tag::Compound(components))
    }

    /// `BlockEntity.applyComponentsFromItemStack` for a container placed
    /// from an item whose component patch is `components` (saved NBT): the
    /// item's name, lock and contents replace the block entity's
    /// (`BaseContainerBlockEntity.applyImplicitComponents`), its loot table
    /// is taken if it has one, and the rest of the patch is kept as the
    /// block entity's `components`. The block entity is then changed
    /// (`setChanged`).
    pub fn apply_container_components(&mut self, pos: BlockPos, components: Option<&Tag>) {
        let Some(store) = self.store_of(self.block(pos)) else { return };
        if self.block_entity(pos).is_none() {
            return;
        }
        let patch = components.and_then(Tag::as_compound);
        let get = |id: &str| patch.and_then(|patch| patch.get(id));
        // `ItemContainerContents.copyInto`: every slot, empty where the
        // item has nothing.
        let mut items = vec![Stack::empty(); store.size()];
        for (index, stack) in get("minecraft:container").and_then(Tag::as_list).into_iter().flatten().filter_map(from_container_slot) {
            if let Some(slot) = items.get_mut(index) {
                *slot = stack;
            }
        }
        let name = get("minecraft:custom_name").cloned();
        let lock = get("minecraft:lock").cloned();
        let loot = get("minecraft:container_loot").and_then(|loot| Some((loot.get("loot_table")?.clone(), loot.get("seed").and_then(Tag::as_i64).unwrap_or(0))));
        // `applyComponents`: the patch less what the block entity read, and
        // less its removals.
        const READ: [&str; 6] = ["minecraft:custom_name", "minecraft:lock", "minecraft:container", "minecraft:container_loot", "minecraft:block_entity_data", "minecraft:block_state"];
        let kept: BTreeMap<String, Tag> = patch.into_iter().flatten().filter(|(id, _)| !READ.contains(&id.as_str()) && !id.starts_with('!')).map(|(id, value)| (id.clone(), value.clone())).collect();
        if let Some(Tag::Compound(entity)) = self.block_entity_mut(pos) {
            match name {
                Some(name) => entity.insert("CustomName".to_owned(), name),
                None => entity.remove("CustomName"),
            };
            match lock {
                Some(lock) => entity.insert("lock".to_owned(), lock),
                None => entity.remove("lock"),
            };
            if let Some((table, seed)) = loot {
                entity.insert("LootTable".to_owned(), table);
                if seed != 0 {
                    entity.insert("LootTableSeed".to_owned(), Tag::Long(seed));
                } else {
                    entity.remove("LootTableSeed");
                }
            }
            entity.insert("components".to_owned(), Tag::Compound(kept));
        }
        self.write_items(pos, &items);
        self.block_entity_changed(pos);
    }

    // ---- hoppers -----------------------------------------------------------------

    fn hopper_cooldown(&self, pos: BlockPos) -> i32 {
        self.block_entity(pos).and_then(|t| t.get("TransferCooldown")).and_then(Tag::as_i64).unwrap_or(-1) as i32
    }

    /// Sets `TransferCooldown`; an unchanged value is no change to save.
    fn set_hopper_cooldown(&mut self, pos: BlockPos, value: i32) {
        if self.block_entity(pos).is_none() || self.hopper_cooldown(pos) == value {
            return;
        }
        if let Some(Tag::Compound(map)) = self.block_entity_mut(pos) {
            map.insert("TransferCooldown".to_owned(), Tag::Int(value));
        }
    }

    /// `HopperBlockEntity.pushItemsTick`. The cooldown counts down, and at
    /// zero or below is zero while the hopper tries to move (the count
    /// below zero is never read, so an idle hopper's stays put).
    pub(super) fn hopper_tick(&mut self, pos: BlockPos) {
        let cooldown = self.hopper_cooldown(pos) - 1;
        self.hopper_ticked.insert(pos, self.game_time);
        if cooldown <= 0 {
            self.set_hopper_cooldown(pos, 0);
            self.hopper_try_move(pos);
        } else {
            self.set_hopper_cooldown(pos, cooldown);
        }
    }

    /// `HopperBlockEntity.tryMoveItems` with `suckInItems`.
    fn hopper_try_move(&mut self, pos: BlockPos) -> bool {
        let state = self.block(pos);
        if self.hopper_cooldown(pos) > 0 || self.registries().blocks.property(state, "enabled") != Some("true") {
            return false;
        }
        let me = ContainerRef::Single(pos, Store::Hopper);
        // Its own `isEmpty` unpacks it.
        self.unpack_container(me);
        let mut changed = false;
        if !self.container_is_empty(me) {
            changed = self.hopper_eject(pos, state);
        }
        if !self.hopper_full(me) {
            changed |= self.hopper_suck(pos);
        }
        if changed {
            self.set_hopper_cooldown(pos, 8);
            self.block_entity_changed(pos);
            return true;
        }
        false
    }

    fn hopper_full(&self, me: ContainerRef) -> bool {
        (0..me.size()).all(|slot| {
            let stack = self.container_item(me, slot);
            !stack.is_empty() && stack.count == self.item_max_stack(&stack)
        })
    }

    /// `HopperBlockEntity.ejectItems`.
    fn hopper_eject(&mut self, pos: BlockPos, state: BlockStateId) -> bool {
        let facing = self.registries().blocks.property(state, "facing").and_then(Direction::from_name).unwrap_or(Direction::Down);
        let Some(target) = self.container_at(pos.relative(facing, 1), true) else { return false };
        // `isFullContainer` reads it, which unpacks it.
        self.unpack_container(target);
        let direction = facing.opposite();
        if self.container_slots(target, direction).into_iter().all(|slot| {
            let stack = self.container_item(target, slot);
            stack.count >= self.item_max_stack(&stack)
        }) {
            return false;
        }
        let me = ContainerRef::Single(pos, Store::Hopper);
        for slot in 0..me.size() {
            let stack = self.container_item(me, slot);
            if stack.is_empty() {
                continue;
            }
            let taken = self.container_remove_item(me, slot, 1);
            let result = self.add_item(Some(me), target, taken, Some(direction));
            if result.is_empty() {
                self.container_set_changed(target);
                return true;
            }
            // The split stack in the slot goes back to its count.
            self.put_item(me, slot, stack.clone());
            if stack.count == 1 {
                self.container_set_item(me, slot, stack);
            }
        }
        false
    }

    /// `HopperBlockEntity.suckInItems` from a container above.
    fn hopper_suck(&mut self, pos: BlockPos) -> bool {
        let Some(source) = self.container_at(pos.above(), true) else { return false };
        self.unpack_container(source);
        let me = ContainerRef::Single(pos, Store::Hopper);
        for slot in self.container_slots(source, Direction::Down) {
            let stack = self.container_item(source, slot);
            if stack.is_empty() {
                continue;
            }
            let taken = self.container_remove_item(source, slot, 1);
            let result = self.add_item(Some(source), me, taken, None);
            if result.is_empty() {
                self.container_set_changed(source);
                return true;
            }
            // The split stack in the slot goes back to its count.
            self.put_item(source, slot, stack.clone());
            if stack.count == 1 {
                self.container_set_item(source, slot, stack);
            }
        }
        false
    }

    /// `HopperBlockEntity.addItem(from, container, stack, direction)`.
    pub(super) fn add_item(&mut self, from: Option<ContainerRef>, into: ContainerRef, mut stack: Stack, direction: Option<Direction>) -> Stack {
        self.unpack_container(into);
        let slots = match direction {
            Some(d) => self.container_slots(into, d),
            None => (0..into.size()).collect(),
        };
        for slot in slots {
            if stack.is_empty() {
                break;
            }
            stack = self.try_move_in_item(from, into, stack, slot, direction);
        }
        stack
    }

    /// `HopperBlockEntity.tryMoveInItem`.
    fn try_move_in_item(&mut self, from: Option<ContainerRef>, into: ContainerRef, mut stack: Stack, slot: usize, direction: Option<Direction>) -> Stack {
        if !self.container_can_place(into, slot, &stack, direction) {
            return stack;
        }
        let current = self.container_item(into, slot);
        let was_empty = self.container_is_empty(into);
        let mut success = false;
        if current.is_empty() {
            self.container_set_item(into, slot, stack);
            stack = Stack::empty();
            success = true;
        } else if current.count <= self.item_max_stack(&current) && current.same_item_same_components(&stack) {
            let space = self.item_max_stack(&stack) - current.count;
            let count = stack.count.min(space);
            stack.count -= count;
            let mut grown = current;
            grown.count += count;
            self.put_item(into, slot, grown);
            success = count > 0;
        }
        if success {
            if let ContainerRef::Single(pos, Store::Hopper) = into {
                if was_empty && self.hopper_cooldown(pos) <= 8 {
                    let mut skip = 0;
                    if let Some(ContainerRef::Single(from_pos, Store::Hopper)) = from {
                        let mine = self.hopper_ticked.get(&pos).copied().unwrap_or(0);
                        let theirs = self.hopper_ticked.get(&from_pos).copied().unwrap_or(0);
                        if mine >= theirs {
                            skip = 1;
                        }
                    }
                    self.set_hopper_cooldown(pos, 8 - skip);
                }
            }
            self.container_set_changed(into);
        }
        stack
    }

    /// `HopperBlock.checkPoweredState`.
    pub(super) fn hopper_check_powered(&mut self, pos: BlockPos, state: BlockStateId) {
        let should_be_on = !self.has_neighbor_signal(pos);
        let on = self.registries().blocks.property(state, "enabled") == Some("true");
        if should_be_on != on {
            let next = self.with(state, "enabled", if should_be_on { "true" } else { "false" });
            self.set_block(pos, next, super::update::CLIENTS, super::update::LIMIT);
        }
    }

    pub(super) fn is_hopper(&self, state: BlockStateId) -> bool {
        self.store_of(state) == Some(Store::Hopper)
    }

    pub(super) fn has_ticker(&self, state: BlockStateId) -> bool {
        self.is_hopper(state)
            || self.store_of(state) == Some(Store::ShulkerBox)
            || self.redstone_kind(state) == Some(Kind::MovingPiston)
            || self.redstone_kind(state) == Some(Kind::DaylightDetector) && self.sky.as_ref().is_some_and(|s| s.has_sky_light)
    }
}

/// A slot of the `container` component (`ItemContainerContents.Slot`): its
/// index and the stack as an `ItemStackTemplate`, the count left out at 1.
fn container_slot(slot: usize, stack: &Stack) -> Tag {
    let mut item = BTreeMap::from([("id".to_owned(), Tag::String(stack.id.clone()))]);
    if stack.count != 1 {
        item.insert("count".to_owned(), Tag::Int(stack.count));
    }
    if let Some(components) = &stack.components {
        item.insert("components".to_owned(), components.clone());
    }
    Tag::Compound(BTreeMap::from([("slot".to_owned(), Tag::Int(slot as i32)), ("item".to_owned(), Tag::Compound(item))]))
}

/// A `container` component slot read back: its index and stack (the item
/// may be its id alone).
fn from_container_slot(slot: &Tag) -> Option<(usize, Stack)> {
    let index = usize::try_from(slot.get("slot")?.as_i64()?).ok()?;
    let item = slot.get("item")?;
    let stack = match item.as_str() {
        Some(id) => Stack::new(id, 1),
        None => {
            let mut stack = Stack::new(item.get("id")?.as_str()?, item.get("count").and_then(Tag::as_i64).unwrap_or(1) as i32);
            stack.components = item.get("components").filter(|c| c.as_compound().is_some_and(|map| !map.is_empty())).cloned();
            stack
        }
    };
    Some((index, stack))
}
