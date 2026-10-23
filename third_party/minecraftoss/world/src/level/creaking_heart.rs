//! Creaking hearts on the server level (26.3 `CreakingHeartBlock`,
//! `CreakingHeartBlockEntity`):
//! - A heart wakes at night (`gameplay/creaking_active`) and sleeps by
//!   day. It is uprooted while the pale oak logs above and below it along
//!   its axis are gone, and roots again once they are back.
//! - About once a second an awake heart without a creaking calls one up
//!   (five tries within sixteen blocks across and eight up or down) when a
//!   player is within thirty-two blocks. Its protector goes again by day,
//!   past thirty-four blocks off, or with a player stuck inside it.
//! - Struck by a player, its creaking makes the heart cry out for five
//!   seconds, the cry moving from the creaking to the heart, and an awake
//!   heart grows two or three resin clumps on its logs.
//! - Broken, its creaking dies with it (twitching first if a player broke
//!   it, which also pops experience from a natural heart). Comparators
//!   read how near its creaking is.
use super::Level;
use minecraftoss_core::nbt::Tag;
use minecraftoss_core::pos::Direction;
use minecraftoss_core::random::RandomSource;
use minecraftoss_core::{BlockPos, BlockStateId};

/// A heart's creaking as the entity world last showed it: its feet, its
/// box, and whether it must stay.
#[derive(Clone, Debug)]
pub struct CreakingLink {
    pub position: [f64; 3],
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub persistent: bool,
}

/// A creaking for the entity world to end: its UUID, and whether it dies
/// (with the player to blame) rather than crumbling away.
#[derive(Clone, Copy, Debug)]
pub struct CreakingRemoval {
    pub uuid: [i32; 4],
    pub killed: bool,
    pub player: Option<u64>,
}

/// What a heart keeps without saving it, and its creaking.
#[derive(Clone, Debug, Default)]
pub(super) struct HeartTicker {
    loaded: bool,
    creaking: Option<[i32; 4]>,
    /// Whether the creaking was found last time (`Either.left`).
    found: bool,
    ticks_existed: i64,
    ticker: i32,
    emitter: i32,
    emitter_target: Option<[f64; 3]>,
    output: i32,
    /// `Creaking.playerStuckCounter`, counted as the heart asks.
    stuck: i32,
}

fn face_name(d: Direction) -> &'static str {
    match d {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}

fn center(min: [f64; 3], max: [f64; 3]) -> [f64; 3] {
    [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0, (min[2] + max[2]) / 2.0]
}

impl Level<'_> {
    /// `gameplay/creaking_active`: night in the Overworld.
    pub fn creaking_active(&self) -> bool {
        let Some(sky) = &self.sky else { return false };
        let clocks = |clock: &str| sky.clocks.get(clock).copied().unwrap_or(0);
        let sample = minecraftoss_core::environment::Sample { clock_ticks: &clocks, rain_level: sky.weather.rain_level, thunder_level: sky.weather.thunder(), biome_weights: None, biome: 0 };
        sky.environment.value_named("gameplay/creaking_active", &sample) == minecraftoss_core::environment::Value::Bool(true)
    }

    /// The hearts loaded and the creaking each keeps, for the entity world.
    pub fn creaking_hearts(&self) -> Vec<((i32, i32, i32), [i32; 4])> {
        self.hearts.iter().filter(|(pos, _)| self.block_entity(**pos).is_some()).filter_map(|(pos, t)| t.creaking.map(|c| ((pos.x, pos.y, pos.z), c))).collect()
    }

    fn heart_state(&self, state: BlockStateId) -> &str {
        self.registries().blocks.property(state, "creaking_heart_state").unwrap_or("uprooted")
    }

    /// `CreakingHeartBlock.hasRequiredLogs`: pale oak logs along its axis on
    /// both sides.
    fn heart_has_logs(&self, state: BlockStateId, pos: BlockPos) -> bool {
        let axis = self.registries().blocks.property(state, "axis").unwrap_or("y").to_owned();
        let (a, b) = match axis.as_str() {
            "x" => (Direction::East, Direction::West),
            "z" => (Direction::South, Direction::North),
            _ => (Direction::Up, Direction::Down),
        };
        [a, b].into_iter().all(|d| {
            let n = self.block(pos.relative(d, 1));
            self.in_block_tag(n, "minecraft:pale_oak_logs") && self.registries().blocks.property(n, "axis") == Some(axis.as_str())
        })
    }

    pub(super) fn in_block_tag(&self, state: BlockStateId, tag: &str) -> bool {
        self.lib.registries.block_tags.require(tag).is_ok_and(|t| self.lib.registries.block_in_tag(state, t))
    }

    /// `CreakingHeartBlock.tick` (scheduled by a shape update): roots again
    /// when its logs are back.
    pub(super) fn creaking_heart_block_tick(&mut self, state: BlockStateId, pos: BlockPos) {
        if self.heart_state(state) == "uprooted" && self.heart_has_logs(state, pos) {
            let next = self.with(state, "creaking_heart_state", if self.creaking_active() { "awake" } else { "dormant" });
            self.set_block_and_update(pos, next);
        }
    }

    fn heart(&mut self, pos: BlockPos) -> &mut HeartTicker {
        let saved = self.block_entity(pos).and_then(|t| t.get("creaking")).and_then(Tag::as_ints).filter(|u| u.len() == 4).map(|u| [u[0], u[1], u[2], u[3]]);
        let t = self.hearts.entry(pos).or_default();
        if !t.loaded {
            // `loadAdditional`: `setCreakingInfo(uuid)`.
            t.loaded = true;
            t.creaking = saved;
        }
        t
    }

    fn set_heart_creaking(&mut self, pos: BlockPos, creaking: Option<[i32; 4]>) {
        let t = self.heart(pos);
        t.creaking = creaking;
        t.found = false;
        t.ticks_existed = 0;
        if let Some(Tag::Compound(map)) = self.block_entity_mut(pos) {
            match creaking {
                Some(u) => map.insert("creaking".to_owned(), Tag::IntArray(u.to_vec())),
                None => map.remove("creaking"),
            };
        }
    }

    /// `getCreakingProtector`: the creaking when the entity world has it; a
    /// UUID that stays unfound for thirty ticks is forgotten.
    fn heart_protector(&mut self, pos: BlockPos) -> Option<super::creaking_heart::CreakingLink> {
        let creaking = self.heart(pos).creaking?;
        if let Some(link) = self.creakings.get(&creaking).cloned() {
            self.heart(pos).found = true;
            return Some(link);
        }
        let t = self.heart(pos);
        if t.found {
            t.found = false;
            t.ticks_existed = 0;
        }
        if t.ticks_existed >= 30 {
            self.set_heart_creaking(pos, None);
        }
        None
    }

    fn heart_distance(&mut self, pos: BlockPos) -> f64 {
        self.heart_protector(pos).map_or(0.0, |c| {
            let (dx, dy, dz) = (c.position[0] - (f64::from(pos.x) + 0.5), c.position[1] - f64::from(pos.y), c.position[2] - (f64::from(pos.z) + 0.5));
            (dx * dx + dy * dy + dz * dz).sqrt()
        })
    }

    /// `computeAnalogOutputSignal`.
    fn heart_signal(&mut self, pos: BlockPos) -> i32 {
        if self.heart(pos).creaking.is_none() || self.heart_protector(pos).is_none() {
            return 0;
        }
        let scaled = self.heart_distance(pos).clamp(0.0, 32.0) / 32.0;
        15 - (scaled * 15.0).floor() as i32
    }

    /// `getAnalogOutputSignal`.
    pub(super) fn heart_output(&self, state: BlockStateId, pos: BlockPos) -> i32 {
        if self.heart_state(state) == "uprooted" {
            0
        } else {
            self.hearts.get(&pos).map_or(0, |t| t.output)
        }
    }

    /// `CreakingHeartBlockEntity.serverTick`.
    pub(super) fn creaking_heart_tick(&mut self, pos: BlockPos) {
        self.heart(pos).ticks_existed += 1;
        let state = self.block(pos);
        let signal = self.heart_signal(pos);
        if self.heart(pos).output != signal {
            self.heart(pos).output = signal;
            let block = self.block_id(state);
            self.update_neighbour_for_output_signal(pos, block);
        }
        let emitter = self.heart(pos).emitter;
        if emitter > 0 {
            if emitter % 10 == 0 && self.heart(pos).emitter_target.is_some() {
                if let Some(c) = self.heart_protector(pos) {
                    self.heart(pos).emitter_target = Some(center(c.min, c.max));
                }
                let target = self.heart(pos).emitter_target.unwrap_or_default();
                let heart = [f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5];
                let progress = 0.2 + 0.8 * f64::from(100 - emitter) / 100.0;
                let at = [0, 1, 2].map(|i| ((heart[i] - target[i]) * progress + target[i]).floor() + 0.5);
                let volume = emitter as f32 / 2.0 / 100.0 + 0.5;
                self.play_sound("block.creaking_heart.hurt", at, volume, 1.0);
            }
            self.heart(pos).emitter -= 1;
        }
        let t = self.heart(pos);
        t.ticker -= 1;
        if t.ticker + 1 >= 0 {
            return;
        }
        self.heart(pos).ticker = self.random.next_i32_bound(5) + 20;
        // `updateCreakingState`.
        let has_creaking = self.heart(pos).creaking.is_some();
        let updated = if !self.heart_has_logs(state, pos) && !has_creaking {
            self.with(state, "creaking_heart_state", "uprooted")
        } else {
            self.with(state, "creaking_heart_state", if self.creaking_active() { "awake" } else { "dormant" })
        };
        if updated != state {
            self.set_block_and_update(pos, updated);
            if self.heart_state(updated) == "uprooted" {
                return;
            }
        }
        if !has_creaking {
            let spawning = self.natural_spawning.as_ref().is_none_or(|n| n.spawn_monsters);
            if self.heart_state(updated) == "awake" && spawning && self.difficulty != 0 {
                let (x, y, z) = (f64::from(pos.x), f64::from(pos.y), f64::from(pos.z));
                let near = self.players.iter().any(|p| (p[0] - x).powi(2) + (p[1] - y).powi(2) + (p[2] - z).powi(2) < 32.0 * 32.0);
                if near {
                    if let Some((uuid, at)) = self.spawn_protector(pos) {
                        self.set_heart_creaking(pos, Some(uuid));
                        self.play_sound("entity.creaking.spawn", at, 1.0, 1.0);
                        self.play_sound("block.creaking_heart.spawn", [x + 0.5, y + 0.5, z + 0.5], 1.0, 1.0);
                    }
                }
            }
        } else if let Some(c) = self.heart_protector(pos) {
            // `playerIsStuckInYou`: a player's eyes in its box, more than
            // four asks running.
            let stuck_now = self.living_players.iter().any(|(p, eye)| {
                let e = [p[0], p[1] + eye, p[2]];
                (0..3).all(|i| e[i] >= c.min[i] && e[i] < c.max[i])
            });
            let t = self.heart(pos);
            t.stuck = if stuck_now { t.stuck + 1 } else { 0 };
            let stuck = t.stuck > 4;
            if (!self.creaking_active() && !c.persistent) || self.heart_distance(pos) > 34.0 || stuck {
                self.heart_remove_protector(pos, None);
            }
        }
    }

    /// `spawnProtector`: `SpawnUtil.trySpawnMob(CREAKING, SPAWNER, pos, 5,
    /// 16, 8, ON_TOP_OF_COLLIDER_NO_LEAVES, true)`. The new creaking's UUID
    /// and feet.
    fn spawn_protector(&mut self, pos: BlockPos) -> Option<([i32; 4], [f64; 3])> {
        for _ in 0..5 {
            let dx = self.random.next_i32_bound(33) - 16;
            let dz = self.random.next_i32_bound(33) - 16;
            let mut at = BlockPos::new(pos.x + dx, pos.y + 8, pos.z + dz);
            // `moveToPossibleSpawnPosition`.
            let mut above = self.block(at);
            let mut found = false;
            for _ in -8..=8 {
                at = at.below();
                let current = self.block(at);
                let top_free = self.block_collision_shape(above, at.above()).is_none();
                let sturdy = self.registries().blocks.is_face_sturdy(current, Direction::Up, minecraftoss_core::SupportType::Full);
                if top_free && !self.in_block_tag(current, "minecraft:leaves") && sturdy {
                    at = at.above();
                    found = true;
                    break;
                }
                above = current;
            }
            if !found {
                continue;
            }
            // `noCollision` for the creaking's box at the cell's centre.
            let (x0, x1) = ((f64::from(at.x) + 0.05).floor() as i32, (f64::from(at.x) + 0.95).floor() as i32);
            let (z0, z1) = ((f64::from(at.z) + 0.05).floor() as i32, (f64::from(at.z) + 0.95).floor() as i32);
            let clear = (x0..=x1).all(|x| (at.y..at.y + 3).all(|y| (z0..=z1).all(|z| {
                let c = BlockPos::new(x, y, z);
                self.block_collision_shape(self.block(c), c).is_none()
            })));
            if !clear {
                continue;
            }
            let feet = [f64::from(at.x) + 0.5, f64::from(at.y), f64::from(at.z) + 0.5];
            let yaw = minecraftoss_core::entity_data::wrap_degrees(self.random.next_f32() * 360.0);
            let home = Tag::IntArray(vec![pos.x, pos.y, pos.z]);
            let tag = self.built_mob_tag("minecraft:creaking", feet, yaw, vec![("home_pos", home)]);
            let uuid = tag.get("UUID").and_then(Tag::as_ints).filter(|u| u.len() == 4).map(|u| [u[0], u[1], u[2], u[3]])?;
            self.spawned.push(tag);
            return Some((uuid, feet));
        }
        None
    }

    /// `removeProtector`: with no damage source its creaking crumbles; with
    /// one it dies (to the player, when one is to blame).
    fn heart_remove_protector(&mut self, pos: BlockPos, killed: Option<Option<u64>>) {
        if self.heart_protector(pos).is_none() {
            return;
        }
        let Some(uuid) = self.heart(pos).creaking else { return };
        self.creaking_removals.push(CreakingRemoval { uuid, killed: killed.is_some(), player: killed.flatten() });
        self.creakings.remove(&uuid);
        self.set_heart_creaking(pos, None);
    }

    /// `preRemoveSideEffects`: the heart's creaking goes with it.
    pub(super) fn creaking_heart_removed(&mut self, pos: BlockPos) {
        self.heart_remove_protector(pos, None);
        self.hearts.remove(&pos);
    }

    /// `CreakingHeartBlock.playerWillDestroy`: the creaking dies to the
    /// player, and a natural heart pops twenty to twenty-four experience for
    /// one not in creative.
    pub fn creaking_heart_will_destroy(&mut self, pos: BlockPos, player: u64, creative: bool) {
        let state = self.block(pos);
        if !self.is_a(state, "CreakingHeartBlock") || self.block_entity(pos).is_none() {
            return;
        }
        self.heart_remove_protector(pos, Some(Some(player)));
        if !creative && self.registries().blocks.property(state, "natural") == Some("true") {
            let amount = self.random.next_i32_bound(5) + 20;
            self.award_experience([f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5], amount);
        }
    }

    /// `onExplosionHit`: a blast kills the heart's creaking.
    pub(super) fn creaking_heart_exploded(&mut self, pos: BlockPos) {
        if self.block_entity(pos).is_some() {
            self.heart_remove_protector(pos, Some(None));
        }
    }

    /// `creakingHurt`: the heart's cry, and resin on an awake heart's logs.
    pub fn creaking_heart_hurt(&mut self, pos: BlockPos) {
        let Some(c) = self.heart_protector(pos) else { return };
        if self.heart(pos).emitter > 0 {
            return;
        }
        if self.heart_state(self.block(pos)) == "awake" {
            let clumps = self.random.next_i32_bound(2) + 2;
            for _ in 0..clumps {
                if let Some(at) = self.spread_resin(pos) {
                    self.play_sound("block.resin.place", [f64::from(at.x) + 0.5, f64::from(at.y) + 0.5, f64::from(at.z) + 0.5], 1.0, 1.0);
                }
            }
        }
        let t = self.heart(pos);
        t.emitter = 100;
        t.emitter_target = Some(center(c.min, c.max));
    }

    fn shuffled_directions(&mut self) -> [Direction; 6] {
        let mut dirs = [Direction::Down, Direction::Up, Direction::North, Direction::South, Direction::West, Direction::East];
        for i in (1..dirs.len()).rev() {
            let j = self.random.next_i32_bound(i as i32 + 1) as usize;
            dirs.swap(i, j);
        }
        dirs
    }

    /// `spreadResin`: through the heart's logs (two deep, at most 64), the
    /// first free face beside one takes a clump.
    fn spread_resin(&mut self, start: BlockPos) -> Option<BlockPos> {
        let mut queue = std::collections::VecDeque::from([(start, 0)]);
        let mut seen = std::collections::HashSet::from([start]);
        let mut count = 0;
        while let Some((pos, depth)) = queue.pop_front() {
            if self.in_block_tag(self.block(pos), "minecraft:pale_oak_logs") {
                for dir in self.shuffled_directions() {
                    let n = pos.relative(dir, 1);
                    let mut state = self.block(n);
                    let face = face_name(dir.opposite());
                    if self.registries().blocks.is_air(state) {
                        state = self.registries().blocks.parse_state("minecraft:resin_clump").ok()?;
                        for d in ["north", "south", "east", "west", "up", "down"] {
                            state = self.with(state, d, "false");
                        }
                    } else if self.name(state) == "minecraft:water" && self.registries().blocks.property(state, "level") == Some("0") {
                        state = self.registries().blocks.parse_state("minecraft:resin_clump").ok()?;
                        for d in ["north", "south", "east", "west", "up", "down"] {
                            state = self.with(state, d, "false");
                        }
                        state = self.with(state, "waterlogged", "true");
                    }
                    if self.name(state) == "minecraft:resin_clump" && self.registries().blocks.property(state, face) != Some("true") {
                        let placed = self.with(state, face, "true");
                        self.set_block_and_update(n, placed);
                        return Some(n);
                    }
                }
            }
            count += 1;
            if count >= 64 {
                return None;
            }
            if depth < 2 {
                for dir in self.shuffled_directions() {
                    let n = pos.relative(dir, 1);
                    if self.in_block_tag(self.block(n), "minecraft:pale_oak_logs") && seen.insert(n) {
                        queue.push_back((n, depth + 1));
                    }
                }
            }
        }
        None
    }
}
