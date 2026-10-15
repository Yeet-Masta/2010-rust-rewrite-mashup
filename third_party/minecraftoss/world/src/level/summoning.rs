//! Golems and the wither built from blocks (26.3 `CarvedPumpkinBlock`,
//! `WitherSkullBlock` and the `BlockPattern`s they search for).
//!
//! - A carved pumpkin or jack o'lantern new to its cell (`onPlace`) looks
//!   for a snow golem (two snow blocks under it), an iron golem (a T of
//!   iron blocks with air at its feet) and a copper golem (any block of
//!   `#copper` under it), in that order (`trySpawnGolem`).
//! - A wither skeleton skull a player places (`setPlacedBy`) looks for the
//!   wither: three skulls on a T of `#wither_summon_base_blocks` with air at
//!   its feet (`checkSpawn`), never in peaceful.
//!
//! The patterns are found in any orientation, upright, upside down or lying
//! (`BlockPattern.find`). The blocks of the shape break without drops, each
//! with its destroy effect (level event 2001), the mob joins the level as a
//! saved tag in `Level::spawned`, and the cells' neighbours are told
//! (`updatePatternBlocks`). The copper golem's block becomes a copper chest
//! of its age, facing as the pumpkin did. Dispensers place pumpkins and
//! wither skulls only where they complete a shape (`DispenseItemBehavior`).

use super::container::Stack;
use super::{update, Level};
use crate::natural_spawner::SpawnLevel;
use minecraftoss_core::nbt::Tag;
use minecraftoss_core::pos::{Axis, Direction};
use minecraftoss_core::tags::TagId;
use minecraftoss_core::{BlockId, BlockPos, BlockStateId};

/// `LevelEvent.PARTICLES_DESTROY_BLOCK`: a block's break sound and pieces.
const DESTROY_BLOCK: i32 = 2001;
/// `WitherBoss.INVULNERABLE_TICKS`.
const WITHER_INVULNERABLE_TICKS: i32 = 220;
/// `WeatheringCopper.WeatherState`'s serialized names, by age.
const WEATHER_STATES: [&str; 4] = ["unaffected", "exposed", "weathered", "oxidized"];

/// What a cell of a pattern wants there.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    /// `' '`: anything.
    Any,
    /// `'~'`: `BlockStateBase.isAir`.
    Air,
    /// `PUMPKINS_PREDICATE`: a carved pumpkin or a jack o'lantern.
    Pumpkin,
    Snow,
    Iron,
    /// `#minecraft:copper`.
    Copper,
    /// A wither skeleton skull, standing or on a wall.
    WitherSkull,
    /// `#minecraft:wither_summon_base_blocks`.
    SoulBase,
}

/// A `BlockPattern` one aisle deep, as `BlockPatternBuilder.aisle` takes
/// it: rows top to bottom, each left to right.
#[derive(Clone, Copy)]
struct Pattern {
    rows: &'static [&'static str],
    /// What `^` and `#` stand for (`where`); `~` is air, ` ` anything.
    head: Cell,
    body: Cell,
}

impl Pattern {
    fn width(&self) -> i32 {
        self.rows[0].len() as i32
    }

    fn height(&self) -> i32 {
        self.rows.len() as i32
    }

    fn cell(&self, right: i32, down: i32) -> Cell {
        match self.rows[down as usize].as_bytes()[right as usize] {
            b'^' => self.head,
            b'#' => self.body,
            b'~' => Cell::Air,
            _ => Cell::Any,
        }
    }
}

/// `getOrCreateSnowGolemFull` and `getOrCreateSnowGolemBase`.
const SNOW_GOLEM: Pattern = Pattern { rows: &["^", "#", "#"], head: Cell::Pumpkin, body: Cell::Snow };
const SNOW_GOLEM_BASE: Pattern = Pattern { rows: &[" ", "#", "#"], ..SNOW_GOLEM };
/// `getOrCreateIronGolemFull` and `getOrCreateIronGolemBase`.
const IRON_GOLEM: Pattern = Pattern { rows: &["~^~", "###", "~#~"], head: Cell::Pumpkin, body: Cell::Iron };
const IRON_GOLEM_BASE: Pattern = Pattern { rows: &["~ ~", "###", "~#~"], ..IRON_GOLEM };
/// `getOrCreateCopperGolemFull` and `getOrCreateCopperGolemBase`.
const COPPER_GOLEM: Pattern = Pattern { rows: &["^", "#"], head: Cell::Pumpkin, body: Cell::Copper };
const COPPER_GOLEM_BASE: Pattern = Pattern { rows: &[" ", "#"], ..COPPER_GOLEM };
/// `WitherSkullBlock.getOrCreateWitherFull` and `getOrCreateWitherBase`.
const WITHER: Pattern = Pattern { rows: &["^^^", "###", "~#~"], head: Cell::WitherSkull, body: Cell::SoulBase };
const WITHER_BASE: Pattern = Pattern { rows: &["   ", "###", "~#~"], ..WITHER };

/// The block tags the patterns test, looked up once a search.
struct PatternTags {
    copper: Option<TagId>,
    soul_base: Option<TagId>,
}

/// `BlockPattern.BlockPatternMatch`: where the shape stands and how it is
/// turned, with the blocks it was made of (the match's `BlockInWorld`
/// cache: what each cell held when it matched).
struct PatternMatch {
    front_top_left: BlockPos,
    forwards: Direction,
    up: Direction,
    width: i32,
    height: i32,
    states: Vec<BlockStateId>,
}

impl PatternMatch {
    /// `getBlock(right, down, 0).getPos()`.
    fn pos(&self, right: i32, down: i32) -> BlockPos {
        translate_and_rotate(self.front_top_left, self.forwards, self.up, right, down, 0)
    }

    /// `getBlock(right, down, 0).getState()`, as it was when it matched.
    fn state(&self, right: i32, down: i32) -> BlockStateId {
        self.states[(down * self.width + right) as usize]
    }
}

/// `BlockPattern.translateAndRotate`: a cell `right`, `down` and `forward`
/// of the origin, with right the cross of forwards and up.
fn translate_and_rotate(origin: BlockPos, forwards: Direction, up: Direction, right: i32, down: i32, forward: i32) -> BlockPos {
    let (fx, fy, fz) = forwards.offset();
    let (ux, uy, uz) = up.offset();
    let (rx, ry, rz) = (fy * uz - fz * uy, fz * ux - fx * uz, fx * uy - fy * ux);
    origin.offset(-ux * down + rx * right + fx * forward, -uy * down + ry * right + fy * forward, -uz * down + rz * right + fz * forward)
}

/// The age of a copper block or chest by its name (`WeatheringCopper.getAge`,
/// a waxed one's through `HoneycombItem.WAX_OFF_BY_BLOCK`): 0 unaffected to
/// 3 oxidized.
fn weather_age(name: &str) -> usize {
    let bare = name.trim_start_matches("minecraft:").trim_start_matches("waxed_");
    ["exposed", "weathered", "oxidized"].iter().position(|age| bare.starts_with(age)).map_or(0, |i| i + 1)
}

impl Level<'_> {
    fn pattern_tags(&self) -> PatternTags {
        let tags = &self.lib.registries.block_tags;
        PatternTags { copper: tags.require("minecraft:copper").ok(), soul_base: tags.require("minecraft:wither_summon_base_blocks").ok() }
    }

    /// The pattern predicates (`BlockInWorld.hasState` and friends).
    fn cell_holds(&self, cell: Cell, state: BlockStateId, tags: &PatternTags) -> bool {
        let in_tag = |tag: Option<TagId>| tag.is_some_and(|tag| self.lib.registries.block_in_tag(state, tag));
        match cell {
            Cell::Any => true,
            Cell::Air => self.registries().blocks.is_air(state),
            Cell::Pumpkin => matches!(self.name(state), "minecraft:carved_pumpkin" | "minecraft:jack_o_lantern"),
            Cell::Snow => self.name(state) == "minecraft:snow_block",
            Cell::Iron => self.name(state) == "minecraft:iron_block",
            Cell::Copper => in_tag(tags.copper),
            Cell::WitherSkull => matches!(self.name(state), "minecraft:wither_skeleton_skull" | "minecraft:wither_skeleton_wall_skull"),
            Cell::SoulBase => in_tag(tags.soul_base),
        }
    }

    /// `BlockPattern.matches`: every cell of the shape turned this way
    /// holds what it wants.
    fn pattern_matches(&self, pattern: &Pattern, origin: BlockPos, forwards: Direction, up: Direction, tags: &PatternTags) -> Option<PatternMatch> {
        let (width, height) = (pattern.width(), pattern.height());
        let mut states = vec![BlockStateId::AIR; (width * height) as usize];
        for right in 0..width {
            for down in 0..height {
                let state = self.block(translate_and_rotate(origin, forwards, up, right, down, 0));
                if !self.cell_holds(pattern.cell(right, down), state, tags) {
                    return None;
                }
                states[(down * width + right) as usize] = state;
            }
        }
        Some(PatternMatch { front_top_left: origin, forwards, up, width, height, states })
    }

    /// `BlockPattern.find`: the first match with its front top left corner
    /// in the cube reaching the pattern's size up and along from `origin`
    /// (x first, then y, then z), trying every forwards and up of
    /// `Direction.values()` at each.
    fn find_pattern(&self, pattern: &Pattern, origin: BlockPos) -> Option<PatternMatch> {
        let tags = self.pattern_tags();
        let reach = pattern.width().max(pattern.height());
        for dz in 0..reach {
            for dy in 0..reach {
                for dx in 0..reach {
                    let corner = origin.offset(dx, dy, dz);
                    for forwards in Direction::ALL {
                        for up in Direction::ALL {
                            if up == forwards || up == forwards.opposite() {
                                continue;
                            }
                            if let Some(found) = self.pattern_matches(pattern, corner, forwards, up, &tags) {
                                return Some(found);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// `CarvedPumpkinBlock.clearPatternBlocks`: each cell of the front
    /// aisle to air (flags 2), with the destroy effect of what it held.
    fn clear_pattern_blocks(&mut self, found: &PatternMatch) {
        for right in 0..found.width {
            for down in 0..found.height {
                let pos = found.pos(right, down);
                self.set_block(pos, BlockStateId::AIR, update::CLIENTS, update::LIMIT);
                self.level_event(DESTROY_BLOCK, pos, i32::from(found.state(right, down).0));
            }
        }
    }

    /// `CarvedPumpkinBlock.updatePatternBlocks`: the cells' neighbours hear
    /// that air came.
    fn update_pattern_blocks(&mut self, found: &PatternMatch) {
        let air = self.block_id(BlockStateId::AIR);
        for right in 0..found.width {
            for down in 0..found.height {
                let pos = found.pos(right, down);
                self.update_neighbors_at(pos, air);
            }
        }
    }

    /// `EntityType.create(level, TRIGGERED)` then `snapTo`: a fresh mob's
    /// saved tag at a point, facing `y_rot` (no `finalizeSpawn`).
    pub(super) fn built_mob_tag(&mut self, kind: &str, at: [f64; 3], y_rot: f32, fields: Vec<(&str, Tag)>) -> Tag {
        let mut tag = self.lib.registries.entities.as_ref().and_then(|c| c.default_tag(kind)).unwrap_or_else(|| Tag::Compound(Default::default()));
        let uuid = SpawnLevel::next_uuid(self);
        minecraftoss_core::entity_data::place(&mut tag, at, y_rot, 0.0, uuid);
        if let Tag::Compound(map) = &mut tag {
            map.insert("id".to_owned(), Tag::String(kind.to_owned()));
            for (key, value) in fields {
                map.insert(key.to_owned(), value);
            }
        }
        tag
    }

    /// `CarvedPumpkinBlock.spawnGolemInWorld`: the shape goes, the golem
    /// stands on the given cell's floor (`snapTo` at its middle, 0.05 up,
    /// facing south) and joins the level, and the cells' neighbours hear.
    fn spawn_golem_in_world(&mut self, found: &PatternMatch, kind: &str, cell: BlockPos, fields: Vec<(&str, Tag)>) -> [f64; 3] {
        self.clear_pattern_blocks(found);
        let at = [f64::from(cell.x) + 0.5, f64::from(cell.y) + 0.05, f64::from(cell.z) + 0.5];
        let tag = self.built_mob_tag(kind, at, 0.0, fields);
        self.spawned.push(tag);
        self.update_pattern_blocks(found);
        at
    }

    /// `CarvedPumpkinBlock.onPlace`: a pumpkin that was not there before
    /// tries to become a golem.
    pub(super) fn carved_pumpkin_on_place(&mut self, state: BlockStateId, pos: BlockPos, old: BlockStateId) {
        if self.block_id(old) != self.block_id(state) {
            self.try_spawn_golem(pos);
        }
    }

    /// `CarvedPumpkinBlock.trySpawnGolem`: a snow golem, else an iron golem
    /// (`setPlayerCreated(true)`), else a copper golem.
    fn try_spawn_golem(&mut self, top: BlockPos) {
        if let Some(found) = self.find_pattern(&SNOW_GOLEM, top) {
            let cell = found.pos(0, 2);
            self.spawn_golem_in_world(&found, "minecraft:snow_golem", cell, Vec::new());
            return;
        }
        if let Some(found) = self.find_pattern(&IRON_GOLEM, top) {
            let cell = found.pos(1, 2);
            self.spawn_golem_in_world(&found, "minecraft:iron_golem", cell, vec![("PlayerCreated", Tag::Byte(1))]);
            return;
        }
        if let Some(found) = self.find_pattern(&COPPER_GOLEM, top) {
            // `getWeatherStateFromPattern`: the copper block's age, then
            // `CopperGolem.spawn` takes it on.
            let weather = WEATHER_STATES[weather_age(self.name(found.state(0, 1)))];
            let cell = found.pos(0, 0);
            let at = self.spawn_golem_in_world(&found, "minecraft:copper_golem", cell, vec![("weather_state", Tag::String(weather.to_owned()))]);
            self.replace_copper_block_with_chest(&found);
            // `CopperGolem.playSpawnSound`.
            self.play_sound("minecraft:entity.copper_golem.spawn", at, 1.0, 1.0);
        }
    }

    /// `CarvedPumpkinBlock.canSpawnGolem`: a golem's body without its head
    /// stands at `top`.
    fn can_spawn_golem(&self, top: BlockPos) -> bool {
        [SNOW_GOLEM_BASE, IRON_GOLEM_BASE, COPPER_GOLEM_BASE].iter().any(|base| self.find_pattern(base, top).is_some())
    }

    /// `CarvedPumpkinBlock.replaceCopperBlockWithChest`: the copper block
    /// under the pumpkin becomes its copper chest (flags 2), facing as the
    /// pumpkin did.
    fn replace_copper_block_with_chest(&mut self, found: &PatternMatch) {
        let blocks = &self.registries().blocks;
        let facing = blocks.property(found.state(0, 0), "facing").and_then(Direction::from_name).unwrap_or(Direction::North);
        let pos = found.pos(0, 1);
        if let Some(chest) = self.copper_chest_from(found.state(0, 1), facing, pos) {
            self.set_block(pos, chest, update::CLIENTS, update::LIMIT);
        }
    }

    /// `CopperChestBlock.getFromCopperBlock`: the chest of a copper block's
    /// age and waxing (`COPPER_TO_COPPER_CHEST_MAPPING`), joining a single
    /// copper chest beside it that faces the same way (`getChestType`),
    /// as the least oxidized of the two (`getLeastOxidizedChestOfConnectedBlocks`).
    fn copper_chest_from(&self, copper: BlockStateId, facing: Direction, pos: BlockPos) -> Option<BlockStateId> {
        let blocks = &self.registries().blocks;
        let name = self.name(copper);
        let chest = blocks.block_by_name(&format!("{}_chest", name.strip_suffix("_block").unwrap_or(name))).or_else(|| blocks.block_by_name("minecraft:copper_chest"))?;
        let copper_chests = self.lib.registries.block_tags.require("minecraft:copper_chests").ok();
        // `candidatePartnerFacing` with `chestCanConnectTo`.
        let partner = |side: Direction| {
            let state = self.block(pos.relative(side, 1));
            let connects = copper_chests.is_some_and(|tag| self.lib.registries.block_in_tag(state, tag)) && blocks.property(state, "type") == Some("single");
            connects.then(|| blocks.property(state, "facing").and_then(Direction::from_name)).flatten()
        };
        let kind = if partner(facing.clockwise()) == Some(facing) {
            "left"
        } else if partner(facing.counter_clockwise()) == Some(facing) {
            "right"
        } else {
            "single"
        };
        let mut state = blocks.block(chest).default_state();
        state = self.with(state, "facing", facing.name());
        state = self.with(state, "type", kind);
        if kind == "single" {
            return Some(state);
        }
        let side = if kind == "left" { facing.clockwise() } else { facing.counter_clockwise() };
        let other = self.block(pos.relative(side, 1));
        if !(self.is_a(state, "CopperChestBlock") && self.is_a(other, "CopperChestBlock")) {
            return Some(state);
        }
        // `unwaxBlock` both when one is waxed and the other is not.
        let waxed = |s: BlockStateId| !self.is_a(s, "WeatheringCopperChestBlock");
        let (mut mine, mut theirs) = (state, other);
        if waxed(state) != waxed(other) {
            mine = self.unwaxed_chest(state).unwrap_or(mine);
            theirs = self.unwaxed_chest(other).unwrap_or(theirs);
        }
        let least = if weather_age(self.name(state)) <= weather_age(self.name(other)) { self.block_id(mine) } else { self.block_id(theirs) };
        Some(self.with_properties_of(least, mine))
    }

    /// `CopperChestBlock.unwaxBlock`: a waxed chest's unwaxed one
    /// (`WAX_OFF_BY_BLOCK`), an unwaxed one as it is.
    fn unwaxed_chest(&self, state: BlockStateId) -> Option<BlockStateId> {
        if self.is_a(state, "WeatheringCopperChestBlock") {
            return Some(state);
        }
        let name = self.name(state);
        let unwaxed = name.strip_prefix("minecraft:waxed_").and_then(|rest| self.registries().blocks.block_by_name(&format!("minecraft:{rest}")))?;
        Some(self.with_properties_of(unwaxed, state))
    }

    /// `Block.withPropertiesOf`: a block's default state with the
    /// properties it shares with `source` copied over.
    fn with_properties_of(&self, block: BlockId, source: BlockStateId) -> BlockStateId {
        let blocks = &self.registries().blocks;
        let mut state = blocks.block(block).default_state();
        for property in blocks.block(blocks.block_of(source)).properties() {
            if let Some(next) = blocks.property(source, &property.name).and_then(|value| blocks.with_property(state, &property.name, value)) {
                state = next;
            }
        }
        state
    }

    /// `Block.setPlacedBy` for a block a player just placed at `pos`: a
    /// wither skeleton skull, on the floor or a wall, may complete a wither
    /// (`WitherSkullBlock`, `WitherWallSkullBlock`).
    pub fn set_placed_by(&mut self, pos: BlockPos) {
        let state = self.block(pos);
        if self.is_a(state, "WitherSkullBlock") || self.is_a(state, "WitherWallSkullBlock") {
            self.check_wither_spawn(pos);
        }
    }

    /// `WitherSkullBlock.checkSpawn`: a wither skull completing the shape,
    /// outside peaceful, makes the wither. It stands on the stem's cell
    /// (0.55 up its middle), facing along the shape's arms, and gathers its
    /// strength first (`makeInvulnerable`: 220 ticks, a third of its health).
    fn check_wither_spawn(&mut self, pos: BlockPos) {
        let state = self.block(pos);
        let skull = matches!(self.name(state), "minecraft:wither_skeleton_skull" | "minecraft:wither_skeleton_wall_skull");
        if !skull || pos.y < self.min_y() || self.difficulty == 0 {
            return;
        }
        let Some(found) = self.find_pattern(&WITHER, pos) else { return };
        self.clear_pattern_blocks(&found);
        let stem = found.pos(1, 2);
        let at = [f64::from(stem.x) + 0.5, f64::from(stem.y) + 0.55, f64::from(stem.z) + 0.5];
        let y_rot = if found.forwards.axis() == Axis::X { 0.0 } else { 90.0 };
        let fields = vec![("Invul", Tag::Int(WITHER_INVULNERABLE_TICKS)), ("Health", Tag::Float(300.0 / 3.0))];
        let tag = self.built_mob_tag("minecraft:wither", at, y_rot, fields);
        self.spawned.push(tag);
        self.update_pattern_blocks(&found);
    }

    /// The cell in front of a dispenser and the way it faces.
    fn dispenser_front(&self, dispenser: BlockPos) -> (BlockPos, Direction) {
        let facing = self.registries().blocks.property(self.block(dispenser), "facing").and_then(Direction::from_name).unwrap_or(Direction::North);
        (dispenser.relative(facing, 1), facing)
    }

    /// The carved pumpkin's dispense behaviour applies: the cell in front
    /// is empty and a golem's body waits there (`canSpawnGolem`).
    pub(super) fn golem_waits_in_front(&self, dispenser: BlockPos) -> bool {
        let (target, _) = self.dispenser_front(dispenser);
        self.registries().blocks.is_air(self.block(target)) && self.can_spawn_golem(target)
    }

    /// The carved pumpkin's dispense behaviour: a pumpkin (facing north)
    /// set in front, whose placing makes the golem.
    pub(super) fn dispense_carved_pumpkin(&mut self, dispenser: BlockPos, mut stack: Stack) -> Stack {
        let (target, _) = self.dispenser_front(dispenser);
        let blocks = &self.registries().blocks;
        if let Some(pumpkin) = blocks.block_by_name("minecraft:carved_pumpkin").map(|b| blocks.block(b).default_state()) {
            self.set_block_and_update(target, pumpkin);
        }
        stack.count -= 1;
        stack
    }

    /// The wither skeleton skull's dispense behaviour applies
    /// (`WitherSkullBlock.canSpawnMob`): the cell in front is empty, two
    /// above the bottom of the world, outside peaceful, with the wither's
    /// body there.
    pub(super) fn wither_waits_in_front(&self, dispenser: BlockPos) -> bool {
        let (target, _) = self.dispenser_front(dispenser);
        self.registries().blocks.is_air(self.block(target)) && target.y >= self.min_y() + 2 && self.difficulty != 0 && self.find_pattern(&WITHER_BASE, target).is_some()
    }

    /// The wither skeleton skull's dispense behaviour: a skull set in front,
    /// turned the way the dispenser faces (`RotationSegment.convertToSegment`),
    /// then `checkSpawn`.
    pub(super) fn dispense_wither_skull(&mut self, dispenser: BlockPos, mut stack: Stack) -> Stack {
        let (target, facing) = self.dispenser_front(dispenser);
        let segment = Direction::BY_2D.iter().position(|&d| d == facing).map_or(0, |i| i * 4);
        let blocks = &self.registries().blocks;
        if let Some(skull) = blocks.block_by_name("minecraft:wither_skeleton_skull").map(|b| blocks.block(b).default_state()) {
            let skull = self.with(skull, "rotation", &segment.to_string());
            self.set_block_and_update(target, skull);
            self.check_wither_spawn(target);
        }
        stack.count -= 1;
        stack
    }
}
