//! Who has a container open, and what the container does about it (26.3
//! `ContainerOpenersCounter` and the block entities' `startOpen`,
//! `stopOpen` and `triggerEvent`): chests (trapped and copper ones too),
//! ender chests and barrels count their openers, play their open and close
//! sounds, send their lids' block event and recheck the count every 5 ticks;
//! a barrel shows `open` while it has openers and a trapped chest gives off
//! its count as a signal. Shulker boxes keep their own count and animate
//! their lid (`ShulkerBoxBlockEntity.updateAnimation`).
//!
//! The counts belong to the block entities in vanilla and are not saved, so
//! they live beside the chunks here and go with the block entity. Not
//! simulated: the `CONTAINER_OPEN`/`CONTAINER_CLOSE` game events (sculk
//! sensors), piglins angered by an opening, and a shulker lid pushing the
//! entities in its way.

use super::physics::Aabb;
use super::Level;
use minecraftoss_core::pos::Direction;
use minecraftoss_core::random::RandomSource;
use minecraftoss_core::{BlockId, BlockPos, BlockStateId};

/// `ContainerOpenersCounter.CHECK_TICK_DELAY`.
const CHECK_TICK_DELAY: i32 = 5;

/// A player with a menu open on block containers, as the openers counters
/// see it (`ContainerUser`); the server sets these before each tick.
#[derive(Clone, Debug)]
pub struct ContainerUser {
    /// The player's bounding box.
    pub bounding_box: Aabb,
    /// `getContainerInteractionRange`: `block_interaction_range` (4.5, and
    /// 5 in creative).
    pub range: f64,
    /// The block entities whose container its menu shows
    /// (`isOwnContainer`): a chest or both halves of a double chest, a
    /// barrel, or its ender inventory's active chest.
    pub open: Vec<BlockPos>,
}

/// A `ContainerOpenersCounter`.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Openers {
    count: i32,
    max_range: f64,
}

/// `ShulkerBoxBlockEntity.AnimationStatus`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LidStatus {
    #[default]
    Closed,
    Opening,
    Opened,
    Closing,
}

/// A shulker box's `openCount` and lid animation.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ShulkerLid {
    open_count: i32,
    status: LidStatus,
    progress: f32,
    progress_old: f32,
}

/// A sound the level played (`Level.playSound` in `SoundSource.BLOCKS`),
/// for the clients.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelSound {
    pub event: &'static str,
    pub position: [f64; 3],
    pub volume: f32,
    pub pitch: f32,
}

/// A block event whose `triggerEvent` asked for the clients to hear of it
/// (`ClientboundBlockEventPacket`): the containers' lids so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SentBlockEvent {
    pub pos: BlockPos,
    pub block: BlockId,
    pub a: i32,
    pub b: i32,
}

/// The block entities that count their openers, by block class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Counted {
    Chest,
    EnderChest,
    Barrel,
}

impl Level<'_> {
    fn counted(&self, state: BlockStateId) -> Option<Counted> {
        if self.is_a(state, "ChestBlock") {
            Some(Counted::Chest)
        } else if self.is_a(state, "EnderChestBlock") {
            Some(Counted::EnderChest)
        } else if self.is_a(state, "BarrelBlock") {
            Some(Counted::Barrel)
        } else {
            None
        }
    }

    /// The block entity at `pos` is opened by a player with `range`
    /// (`Container.startOpen`). Containers without a counter take no note.
    pub fn start_open(&mut self, pos: BlockPos, range: f64) {
        if self.block_entity(pos).is_none() {
            return;
        }
        let state = self.block(pos);
        if self.counted(state).is_some() {
            self.increment_openers(pos, state, range);
        } else if self.is_a(state, "ShulkerBoxBlock") {
            self.shulker_start_open(pos, state);
        }
    }

    /// The player closes the block entity at `pos` (`Container.stopOpen`).
    pub fn stop_open(&mut self, pos: BlockPos) {
        if self.block_entity(pos).is_none() {
            return;
        }
        let state = self.block(pos);
        if self.counted(state).is_some() {
            self.decrement_openers(pos, state);
        } else if self.is_a(state, "ShulkerBoxBlock") {
            self.shulker_stop_open(pos, state);
        }
    }

    /// `ContainerOpenersCounter.getOpenerCount` (`ChestBlockEntity.getOpenCount`).
    pub fn opener_count(&self, pos: BlockPos) -> i32 {
        self.openers.get(&pos).map_or(0, |o| o.count)
    }

    /// `ContainerOpenersCounter.incrementOpeners`.
    fn increment_openers(&mut self, pos: BlockPos, state: BlockStateId, range: f64) {
        let previous = self.opener_count(pos);
        self.openers.entry(pos).or_default().count = previous + 1;
        if previous == 0 {
            self.on_open(pos, state);
            self.schedule_block_tick(pos, state, CHECK_TICK_DELAY);
        }
        self.opener_count_changed(pos, state, previous, previous + 1);
        let openers = self.openers.entry(pos).or_default();
        openers.max_range = openers.max_range.max(range);
    }

    /// `ContainerOpenersCounter.decrementOpeners`.
    fn decrement_openers(&mut self, pos: BlockPos, state: BlockStateId) {
        let previous = self.opener_count(pos);
        let openers = self.openers.entry(pos).or_default();
        openers.count = previous - 1;
        if openers.count == 0 {
            openers.max_range = 0.0;
            self.on_close(pos, state);
        }
        self.opener_count_changed(pos, state, previous, previous - 1);
    }

    /// `ContainerOpenersCounter.recheckOpeners`, the counted containers'
    /// scheduled tick (`ChestBlock.tick`, `BarrelBlock.tick`,
    /// `EnderChestBlock.tick`): the players near enough whose menu shows
    /// the container are its openers.
    pub(super) fn recheck_openers(&mut self, pos: BlockPos, state: BlockStateId) {
        if self.block_entity(pos).is_none() || self.counted(state).is_none() {
            return;
        }
        let Openers { count: previous, max_range } = self.openers.get(&pos).copied().unwrap_or(Openers { count: 0, max_range: 0.0 });
        // `getEntitiesWithContainerOpen`: the block's box grown by the
        // largest reach seen, and four more.
        let around = Aabb::new(f64::from(pos.x), f64::from(pos.y), f64::from(pos.z), f64::from(pos.x) + 1.0, f64::from(pos.y) + 1.0, f64::from(pos.z) + 1.0).inflate(max_range + 4.0);
        let users: Vec<f64> = self.container_users.iter().filter(|u| u.bounding_box.intersects(&around) && u.open.contains(&pos)).map(|u| u.range).collect();
        let count = users.len() as i32;
        self.openers.entry(pos).or_default().max_range = users.iter().copied().fold(0.0, f64::max);
        if previous != count {
            if count != 0 && previous == 0 {
                self.on_open(pos, state);
            } else if count == 0 {
                self.on_close(pos, state);
            }
            self.openers.entry(pos).or_default().count = count;
        }
        self.opener_count_changed(pos, state, previous, count);
        if count > 0 {
            self.schedule_block_tick(pos, state, CHECK_TICK_DELAY);
        }
    }

    /// The counter's `onOpen`: the open sound, and a barrel's `open`.
    fn on_open(&mut self, pos: BlockPos, state: BlockStateId) {
        match self.counted(state) {
            Some(Counted::Chest) => self.chest_sound(pos, state, true),
            Some(Counted::EnderChest) => self.container_sound("minecraft:block.ender_chest.open", Self::centre(pos)),
            Some(Counted::Barrel) => {
                self.barrel_sound(pos, state, "minecraft:block.barrel.open");
                self.barrel_update_state(pos, state, true);
            }
            None => {}
        }
    }

    /// The counter's `onClose`.
    fn on_close(&mut self, pos: BlockPos, state: BlockStateId) {
        match self.counted(state) {
            Some(Counted::Chest) => self.chest_sound(pos, state, false),
            Some(Counted::EnderChest) => self.container_sound("minecraft:block.ender_chest.close", Self::centre(pos)),
            Some(Counted::Barrel) => {
                self.barrel_sound(pos, state, "minecraft:block.barrel.close");
                self.barrel_update_state(pos, state, false);
            }
            None => {}
        }
    }

    /// The counter's `openerCountChanged`: chests and ender chests tell
    /// their lids (block event 1 with the count,
    /// `ChestBlockEntity.signalOpenCount`); a trapped chest whose count
    /// moved updates its neighbours and the block below
    /// (`TrappedChestBlockEntity.signalOpenCount`).
    fn opener_count_changed(&mut self, pos: BlockPos, state: BlockStateId, previous: i32, current: i32) {
        match self.counted(state) {
            Some(Counted::Chest) => {
                let block = self.block_id(state);
                self.block_event(pos, block, 1, current);
                if previous != current && self.is_a(state, "TrappedChestBlock") {
                    self.update_neighbors_at(pos, block);
                    self.update_neighbors_at(pos.below(), block);
                }
            }
            Some(Counted::EnderChest) => {
                let block = self.block_id(state);
                self.block_event(pos, block, 1, current);
            }
            Some(Counted::Barrel) | None => {}
        }
    }

    fn centre(pos: BlockPos) -> [f64; 3] {
        [f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5]
    }

    /// `level.playSound(null, x, y, z, event, BLOCKS, 0.5F, random * 0.1F + 0.9F)`,
    /// the containers' open and close sounds.
    fn container_sound(&mut self, event: &'static str, position: [f64; 3]) {
        let pitch = self.random.next_f32() * 0.1 + 0.9;
        self.sounds.push(LevelSound { event, position, volume: 0.5, pitch });
    }

    /// `ChestBlockEntity.playSound`: a double chest sounds once, from its
    /// right half at the middle of the two; the left half is silent. Copper
    /// chests creak by their weathering (`CopperChestBlock.getHingeSound`).
    fn chest_sound(&mut self, pos: BlockPos, state: BlockStateId, open: bool) {
        let blocks = &self.registries().blocks;
        let kind = blocks.property(state, "type").unwrap_or("single");
        if kind == "left" {
            return;
        }
        let mut position = Self::centre(pos);
        if kind == "right" {
            // `getConnectedDirection` of a right half.
            let facing = blocks.property(state, "facing").and_then(Direction::from_name).unwrap_or(Direction::North);
            let (dx, _, dz) = facing.counter_clockwise().offset();
            position[0] += f64::from(dx) * 0.5;
            position[2] += f64::from(dz) * 0.5;
        }
        let name = self.name(state);
        let copper = name.ends_with("copper_chest");
        let event = match (copper, name.contains("weathered_"), name.contains("oxidized_"), open) {
            (false, _, _, true) => "minecraft:block.chest.open",
            (false, _, _, false) => "minecraft:block.chest.close",
            (true, true, _, true) => "minecraft:block.copper_chest_weathered.open",
            (true, true, _, false) => "minecraft:block.copper_chest_weathered.close",
            (true, false, true, true) => "minecraft:block.copper_chest_oxidized.open",
            (true, false, true, false) => "minecraft:block.copper_chest_oxidized.close",
            (true, false, false, true) => "minecraft:block.copper_chest.open",
            (true, false, false, false) => "minecraft:block.copper_chest.close",
        };
        self.container_sound(event, position);
    }

    /// `BarrelBlockEntity.playSound`: from the middle of its open face.
    fn barrel_sound(&mut self, pos: BlockPos, state: BlockStateId, event: &'static str) {
        let facing = self.registries().blocks.property(state, "facing").and_then(Direction::from_name).unwrap_or(Direction::Up);
        let (dx, dy, dz) = facing.offset();
        let centre = Self::centre(pos);
        self.container_sound(event, [centre[0] + f64::from(dx) / 2.0, centre[1] + f64::from(dy) / 2.0, centre[2] + f64::from(dz) / 2.0]);
    }

    /// `BarrelBlockEntity.updateBlockState`: `setBlockAndUpdate` with `open`.
    fn barrel_update_state(&mut self, pos: BlockPos, state: BlockStateId, open: bool) {
        let next = self.with(state, "open", if open { "true" } else { "false" });
        self.set_block_and_update(pos, next);
    }

    // ---- shulker boxes -----------------------------------------------------------

    /// `ShulkerBoxBlockEntity.startOpen`: the count goes to the lid as
    /// block event 1, and the first opener hears it open.
    fn shulker_start_open(&mut self, pos: BlockPos, state: BlockStateId) {
        let lid = self.shulker_lids.entry(pos).or_default();
        lid.open_count = lid.open_count.max(0) + 1;
        let count = lid.open_count;
        let block = self.block_id(state);
        self.block_event(pos, block, 1, count);
        if count == 1 {
            self.container_sound("minecraft:block.shulker_box.open", Self::centre(pos));
        }
    }

    /// `ShulkerBoxBlockEntity.stopOpen`.
    fn shulker_stop_open(&mut self, pos: BlockPos, state: BlockStateId) {
        let lid = self.shulker_lids.entry(pos).or_default();
        lid.open_count -= 1;
        let count = lid.open_count;
        let block = self.block_id(state);
        self.block_event(pos, block, 1, count);
        if count <= 0 {
            self.container_sound("minecraft:block.shulker_box.close", Self::centre(pos));
        }
    }

    /// A shulker box's lid: `getAnimationStatus`.
    pub fn shulker_lid(&self, pos: BlockPos) -> LidStatus {
        self.shulker_lids.get(&pos).map_or(LidStatus::Closed, |lid| lid.status)
    }

    /// `ShulkerBoxBlock.canOpen`: a lid that is not closed opens; a closed
    /// one needs the half block in front of it free of collisions
    /// (`Shulker.getProgressDeltaAabb(1, facing, 0, 0.5, bottom centre)`,
    /// deflated by 1e-6).
    pub fn shulker_box_can_open(&self, pos: BlockPos) -> bool {
        if self.shulker_lid(pos) != LidStatus::Closed {
            return true;
        }
        let facing = self.registries().blocks.property(self.block(pos), "facing").and_then(Direction::from_name).unwrap_or(Direction::Up);
        let (dx, dy, dz) = facing.offset();
        // The unit box on the bottom centre, grown 0.5 towards the facing
        // and shrunk by 1 from the other side: the slab beyond the face.
        let step = [f64::from(dx), f64::from(dy), f64::from(dz)];
        let mut min = [-0.5, 0.0, -0.5];
        let mut max = [0.5, 1.0, 0.5];
        for axis in 0..3 {
            if step[axis] > 0.0 {
                max[axis] += step[axis] * 0.5;
                min[axis] += step[axis];
            } else if step[axis] < 0.0 {
                min[axis] += step[axis] * 0.5;
                max[axis] += step[axis];
            }
        }
        let bottom = [f64::from(pos.x) + 0.5, f64::from(pos.y), f64::from(pos.z) + 0.5];
        let lid = Aabb::new(min[0] + bottom[0], min[1] + bottom[1], min[2] + bottom[2], max[0] + bottom[0], max[1] + bottom[1], max[2] + bottom[2]).inflate(-1.0e-6);
        self.no_block_collision(&lid)
    }

    /// `ShulkerBoxBlockEntity.triggerEvent` for event 1: the count, and the
    /// lid opening at 1 or closing at 0.
    fn shulker_trigger_event(&mut self, pos: BlockPos, count: i32) {
        let lid = self.shulker_lids.entry(pos).or_default();
        lid.open_count = count;
        if count == 0 {
            lid.status = LidStatus::Closing;
        }
        if count == 1 {
            lid.status = LidStatus::Opening;
        }
    }

    /// `ShulkerBoxBlockEntity.tick` (`updateAnimation`): the lid moves a
    /// tenth a tick, and its neighbours hear when it starts and stops
    /// (`doNeighborUpdates`).
    pub(super) fn shulker_tick(&mut self, pos: BlockPos) {
        let Some(mut lid) = self.shulker_lids.get(&pos).copied() else { return };
        let state = self.block(pos);
        lid.progress_old = lid.progress;
        let mut updates = 0;
        match lid.status {
            LidStatus::Closed => lid.progress = 0.0,
            LidStatus::Opening => {
                lid.progress += 0.1;
                if lid.progress_old == 0.0 {
                    updates += 1;
                }
                if lid.progress >= 1.0 {
                    lid.status = LidStatus::Opened;
                    lid.progress = 1.0;
                    updates += 1;
                }
            }
            LidStatus::Opened => lid.progress = 1.0,
            LidStatus::Closing => {
                lid.progress -= 0.1;
                if lid.progress_old == 1.0 {
                    updates += 1;
                }
                if lid.progress <= 0.0 {
                    lid.status = LidStatus::Closed;
                    lid.progress = 0.0;
                    updates += 1;
                }
            }
        }
        self.shulker_lids.insert(pos, lid);
        for _ in 0..updates {
            self.update_neighbour_shapes(state, pos, super::update::ALL, super::update::LIMIT);
            let block = self.block_id(state);
            self.update_neighbors_at(pos, block);
        }
    }

    /// `triggerEvent` of the containers' block entities: event 1 sets a
    /// chest's or ender chest's lid (on the clients) or a shulker box's
    /// count and lid. Whether the clients hear of it.
    pub(super) fn container_trigger_event(&mut self, state: BlockStateId, pos: BlockPos, a: i32, b: i32) -> bool {
        if a != 1 || self.block_entity(pos).is_none() {
            return false;
        }
        if self.is_a(state, "ShulkerBoxBlock") {
            self.shulker_trigger_event(pos, b);
            return true;
        }
        matches!(self.counted(state), Some(Counted::Chest | Counted::EnderChest))
    }

    /// The block entity at `pos` was removed (broken, replaced, or its
    /// chunk unloaded): its openers and lid go with it, and a menu watching
    /// it learns it is gone.
    pub(super) fn block_entity_removed(&mut self, pos: BlockPos) {
        self.openers.remove(&pos);
        self.shulker_lids.remove(&pos);
        self.brewing_unsaved.remove(&pos);
        if let Some(removed) = self.watched_block_entities.get_mut(&pos) {
            *removed = true;
        }
    }

    /// A menu shows the block entity at `pos` from now on: see
    /// [`Self::block_entity_still_there`].
    pub fn watch_block_entity(&mut self, pos: BlockPos) {
        self.watched_block_entities.insert(pos, false);
    }

    /// The menu showing the block entity at `pos` closed.
    pub fn unwatch_block_entity(&mut self, pos: BlockPos) {
        self.watched_block_entities.remove(&pos);
    }

    /// `level.getBlockEntity(pos) == blockEntity` for a watched block
    /// entity: it is there and was not removed since it was watched.
    pub fn block_entity_still_there(&self, pos: BlockPos) -> bool {
        self.watched_block_entities.get(&pos) == Some(&false) && self.block_entity(pos).is_some()
    }

    /// `Level.playSound` in `SoundSource.BLOCKS`, from no player.
    pub fn play_sound(&mut self, event: &'static str, position: [f64; 3], volume: f32, pitch: f32) {
        self.sounds.push(LevelSound { event, position, volume, pitch });
    }

    /// The sounds played since the last call.
    pub fn take_sounds(&mut self) -> Vec<LevelSound> {
        std::mem::take(&mut self.sounds)
    }

    /// `Level.levelEvent(id, pos, data)`, from no player: for the clients
    /// near, whose `LevelEventHandler` plays its sound or particles.
    pub fn level_event(&mut self, id: i32, pos: BlockPos, data: i32) {
        self.level_events.push((pos, id, data));
    }

    /// The level events since the last call: position, event id, data.
    pub fn take_level_events(&mut self) -> Vec<(BlockPos, i32, i32)> {
        std::mem::take(&mut self.level_events)
    }

    /// The block events the clients hear of, since the last call.
    pub fn take_sent_block_events(&mut self) -> Vec<SentBlockEvent> {
        std::mem::take(&mut self.sent_block_events)
    }
}
