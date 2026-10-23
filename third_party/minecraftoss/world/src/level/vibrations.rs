//! Vibrations and the sculk sensors that pick them up (26.3 `GameEvent`,
//! `VibrationSystem`, `VibrationSelector`, `SculkSensorBlock`,
//! `CalibratedSculkSensorBlock`, `SculkSensorBlockEntity`).
//!
//! - A game event is heard by every inactive sensor within its range (8
//!   blocks, a calibrated one's 16) unless wool stands in the way on every
//!   side, or the source steps carefully for a quiet event. Of what a
//!   sensor hears in one tick, the nearest wins (on a tie, the louder
//!   kind); it chooses on the next tick.
//! - The vibration travels a tick a block, then the sensor activates: its
//!   power the nearer the stronger, its comparator output the event's
//!   frequency (1 a step, 15 an explosion); a calibrated one heeds only the
//!   frequency fed into its back. Active for 30 ticks (10), then 10 of
//!   cooldown.
use super::Level;
use minecraftoss_core::pos::Direction;
use minecraftoss_core::random::RandomSource;
use minecraftoss_core::{BlockPos, BlockStateId};

/// `VibrationSystem.VIBRATION_FREQUENCY_FOR_EVENT`: 0 for an event no
/// vibration listener hears (`#vibrations` is the rest).
pub fn frequency(event: &str) -> i32 {
    match event {
        "step" | "swim" | "flap" => 1,
        "projectile_land" | "hit_ground" | "splash" | "bounce" => 2,
        "item_interact_finish" | "projectile_shoot" | "instrument_play" => 3,
        "entity_action" | "elytra_glide" | "unequip" => 4,
        "entity_dismount" | "equip" => 5,
        "entity_interact" | "shear" | "entity_mount" => 6,
        "entity_damage" => 7,
        "drink" | "eat" => 8,
        "container_close" | "block_close" | "block_deactivate" | "block_detach" => 9,
        "container_open" | "block_open" | "block_activate" | "block_attach" | "prime_fuse" | "note_block_play" => 10,
        "block_change" => 11,
        "block_destroy" | "fluid_pickup" => 12,
        "block_place" | "fluid_place" => 13,
        "entity_place" | "lightning_strike" | "teleport" => 14,
        "entity_die" | "explode" => 15,
        _ => event.strip_prefix("resonate_").and_then(|n| n.parse().ok()).unwrap_or(0),
    }
}

/// `#ignore_vibrations_sneaking`.
fn quiet_when_careful(event: &str) -> bool {
    matches!(event, "hit_ground" | "projectile_shoot" | "step" | "swim" | "item_interact_start" | "item_interact_finish")
}

/// What set a game event off, as the listeners judge it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EventSource {
    pub entity: Option<u64>,
    /// `isSteppingCarefully` (sneaking).
    pub careful: bool,
    /// `dampensVibrations` (a wool item, a warden).
    pub dampens: bool,
}

/// A vibration a listener heard (`VibrationInfo`).
#[derive(Clone, Debug, PartialEq)]
pub struct Vibration {
    pub event: &'static str,
    pub distance: f32,
    pub origin: [f64; 3],
    pub source: Option<u64>,
}

/// A listener's state (`VibrationSystem.Data` with its selector).
#[derive(Clone, Debug, Default)]
pub(super) struct VibrationListener {
    current: Option<Vibration>,
    travel: i32,
    candidate: Option<(Vibration, i64)>,
    /// `SculkSensorBlockEntity.lastVibrationFrequency`.
    last_frequency: i32,
}

/// A vibration on its way, for the client's particle
/// (`VibrationParticleOption`): from where, to which block, how long.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VibrationTrail {
    pub from: [f64; 3],
    pub to: [f64; 3],
    pub ticks: i32,
}

/// `BlockGetter.traverseBlocks`: every block the segment passes through,
/// in order, until `visit` says stop.
fn traverse(from: [f64; 3], to: [f64; 3], mut visit: impl FnMut(BlockPos) -> bool) -> bool {
    if from == to {
        return false;
    }
    let lerp = |t: f64, a: f64, b: f64| a + t * (b - a);
    let end = [lerp(-1.0e-7, to[0], from[0]), lerp(-1.0e-7, to[1], from[1]), lerp(-1.0e-7, to[2], from[2])];
    let start = [lerp(-1.0e-7, from[0], to[0]), lerp(-1.0e-7, from[1], to[1]), lerp(-1.0e-7, from[2], to[2])];
    let mut block = [start[0].floor() as i32, start[1].floor() as i32, start[2].floor() as i32];
    if visit(BlockPos::new(block[0], block[1], block[2])) {
        return true;
    }
    let mut sign = [0; 3];
    let mut delta = [f64::MAX; 3];
    let mut next = [f64::MAX; 3];
    for axis in 0..3 {
        let d = end[axis] - start[axis];
        sign[axis] = if d > 0.0 { 1 } else if d < 0.0 { -1 } else { 0 };
        if sign[axis] != 0 {
            delta[axis] = f64::from(sign[axis]) / d;
            let frac = start[axis] - start[axis].floor();
            next[axis] = delta[axis] * if sign[axis] > 0 { 1.0 - frac } else { frac };
        }
    }
    while next.iter().any(|&t| t <= 1.0) {
        let axis = if next[0] < next[1] {
            if next[0] < next[2] { 0 } else { 2 }
        } else if next[1] < next[2] {
            1
        } else {
            2
        };
        block[axis] += sign[axis];
        next[axis] += delta[axis];
        if visit(BlockPos::new(block[0], block[1], block[2])) {
            return true;
        }
    }
    false
}

impl Level<'_> {
    /// A sculk sensor's range, how long it stays active, and whether it is
    /// the calibrated kind.
    pub(super) fn sensor_kind(&self, state: BlockStateId) -> Option<(i32, i32, bool)> {
        match self.name(state) {
            "minecraft:sculk_sensor" => Some((8, 30, false)),
            "minecraft:calibrated_sculk_sensor" => Some((16, 10, true)),
            _ => None,
        }
    }

    fn sensor_phase(&self, state: BlockStateId) -> &str {
        self.registries().blocks.property(state, "sculk_sensor_phase").unwrap_or("inactive")
    }

    /// `Listener.isOccluded`: wool fills the line from every nudged side.
    fn occluded(&self, origin: [f64; 3], dest: [f64; 3]) -> bool {
        let centre = |p: [f64; 3]| [p[0].floor() + 0.5, p[1].floor() + 0.5, p[2].floor() + 0.5];
        let (from, to) = (centre(origin), centre(dest));
        Direction::ALL.iter().all(|d| {
            let (dx, dy, dz) = d.offset();
            let nudged = [from[0] + f64::from(dx) * 1.0e-5, from[1] + f64::from(dy) * 1.0e-5, from[2] + f64::from(dz) * 1.0e-5];
            traverse(nudged, to, |pos| self.in_block_tag(self.block(pos), "minecraft:occludes_vibration_signals"))
        })
    }

    /// `ServerLevel.gameEvent`: every listener in range that may hear it
    /// takes it as a candidate (`Listener.handleGameEvent`).
    pub fn game_event(&mut self, event: &'static str, at: [f64; 3], source: EventSource, affected: Option<BlockStateId>) {
        let frequency = frequency(event);
        // `User.isValidVibration`.
        if frequency == 0 || source.dampens || source.careful && quiet_when_careful(event) {
            return;
        }
        if affected.is_some_and(|s| self.in_block_tag(s, "minecraft:dampens_vibrations")) {
            return;
        }
        let event_block = BlockPos::new(at[0].floor() as i32, at[1].floor() as i32, at[2].floor() as i32);
        let reach = 16;
        let (cx, cz) = (event_block.x >> 4, event_block.z >> 4);
        let mut heard = Vec::new();
        for chunk_x in cx - 1..=cx + 1 {
            for chunk_z in cz - 1..=cz + 1 {
                let Some(chunk) = self.chunk(minecraftoss_core::ChunkPos::new(chunk_x, chunk_z)) else { continue };
                for &(x, y, z) in chunk.block_entities.entities.keys() {
                    if (x - event_block.x).abs() > reach || (y - event_block.y).abs() > reach || (z - event_block.z).abs() > reach {
                        continue;
                    }
                    heard.push(BlockPos::new(x, y, z));
                }
            }
        }
        for pos in heard {
            let state = self.block(pos);
            let Some((radius, _, calibrated)) = self.sensor_kind(state) else { continue };
            let dest = [f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5];
            let (dx, dy, dz) = (dest[0] - at[0], dest[1] - at[1], dest[2] - at[2]);
            let distance_sq = dx * dx + dy * dy + dz * dz;
            if distance_sq > f64::from(radius * radius) {
                continue;
            }
            if self.vibration_listeners.get(&pos).is_some_and(|l| l.current.is_some()) {
                continue;
            }
            // `SculkSensorBlockEntity.VibrationUser.canReceiveVibration`:
            // not its own placing or breaking; only while inactive; a
            // calibrated one only at the frequency fed into its back.
            if pos == event_block && matches!(event, "block_destroy" | "block_place") {
                continue;
            }
            if self.sensor_phase(state) != "inactive" {
                continue;
            }
            if calibrated {
                let back = self.registries().blocks.property(state, "facing").and_then(Direction::from_name).map(Direction::opposite);
                let fed = back.map_or(0, |d| self.signal(pos.relative(d, 1), d));
                if fed != 0 && fed != frequency {
                    continue;
                }
            }
            if self.occluded(at, dest) {
                continue;
            }
            // `VibrationSelector.addCandidate`: within a tick, the nearest,
            // then the louder.
            let vibration = Vibration { event, distance: distance_sq.sqrt() as f32, origin: at, source: source.entity };
            let now = self.game_time;
            let listener = self.vibration_listeners.entry(pos).or_default();
            let replace = match &listener.candidate {
                None => true,
                Some((old, tick)) if *tick == now => vibration.distance < old.distance || vibration.distance == old.distance && frequency > self::frequency(old.event),
                Some(_) => false,
            };
            if replace {
                listener.candidate = Some((vibration, now));
            }
        }
    }

    /// `VibrationSystem.Ticker.tick` for a sculk sensor: a candidate from
    /// an earlier tick sets off, travels a tick a block, then arrives.
    pub(super) fn sculk_sensor_tick(&mut self, pos: BlockPos) {
        let now = self.game_time;
        let Some(listener) = self.vibration_listeners.get_mut(&pos) else { return };
        if listener.current.is_none() {
            if let Some((vibration, tick)) = listener.candidate.take() {
                if tick < now {
                    listener.travel = vibration.distance.floor() as i32;
                    let to = [f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5];
                    self.vibration_trails.push(VibrationTrail { from: vibration.origin, to, ticks: listener.travel });
                    listener.current = Some(vibration);
                } else {
                    listener.candidate = Some((vibration, tick));
                }
            }
        }
        let Some(listener) = self.vibration_listeners.get_mut(&pos) else { return };
        if listener.current.is_none() {
            return;
        }
        listener.travel = (listener.travel - 1).max(0);
        if listener.travel > 0 {
            return;
        }
        let Some(vibration) = listener.current.take() else { return };
        // `receiveVibration`, the distance between the blocks.
        let origin = BlockPos::new(vibration.origin[0].floor() as i32, vibration.origin[1].floor() as i32, vibration.origin[2].floor() as i32);
        let (dx, dy, dz) = (f64::from(origin.x - pos.x), f64::from(origin.y - pos.y), f64::from(origin.z - pos.z));
        let distance = (dx * dx + dy * dy + dz * dz).sqrt() as f32;
        let state = self.block(pos);
        let Some((radius, active_ticks, _)) = self.sensor_kind(state) else { return };
        if self.sensor_phase(state) != "inactive" {
            return;
        }
        let frequency = frequency(vibration.event);
        if let Some(listener) = self.vibration_listeners.get_mut(&pos) {
            listener.last_frequency = frequency;
        }
        // `getRedstoneStrengthForDistance`.
        let power = (15 - (15.0 / f64::from(radius) * f64::from(distance)).floor() as i32).max(1);
        self.sensor_activate(state, pos, power, active_ticks, vibration.source);
    }

    /// `SculkSensorBlock.activate`.
    fn sensor_activate(&mut self, state: BlockStateId, pos: BlockPos, power: i32, active_ticks: i32, source: Option<u64>) {
        let blocks = &self.registries().blocks;
        let active = blocks.with_property(state, "sculk_sensor_phase", "active").and_then(|s| blocks.with_property(s, "power", &power.to_string())).unwrap_or(state);
        self.set_block_and_update(pos, active);
        self.schedule_block_tick(pos, active, active_ticks);
        self.sensor_update_neighbours(pos, active);
        self.game_event("sculk_sensor_tendrils_clicking", [f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5], EventSource { entity: source, ..Default::default() }, None);
        if !self.flag(state, "waterlogged") {
            let pitch = self.random.next_f32() * 0.2 + 0.8;
            self.play_sound("block.sculk_sensor.clicking", [f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5], 1.0, pitch);
        }
    }

    fn sensor_update_neighbours(&mut self, pos: BlockPos, state: BlockStateId) {
        let block = self.block_id(state);
        self.update_neighbors_at(pos, block);
        self.update_neighbors_at(pos.relative(Direction::Down, 1), block);
    }

    /// `SculkSensorBlock.tick`: an active sensor cools down (its power
    /// gone), a cooling one goes quiet again.
    pub(super) fn sculk_sensor_block_tick(&mut self, state: BlockStateId, pos: BlockPos) {
        match self.sensor_phase(state) {
            "active" => {
                let blocks = &self.registries().blocks;
                let cooling = blocks.with_property(state, "sculk_sensor_phase", "cooldown").and_then(|s| blocks.with_property(s, "power", "0")).unwrap_or(state);
                self.set_block_and_update(pos, cooling);
                self.schedule_block_tick(pos, cooling, 10);
                self.sensor_update_neighbours(pos, cooling);
            }
            "cooldown" => {
                let quiet = self.registries().blocks.with_property(state, "sculk_sensor_phase", "inactive").unwrap_or(state);
                self.set_block_and_update(pos, quiet);
                if !self.flag(state, "waterlogged") {
                    let pitch = self.random.next_f32() * 0.2 + 0.8;
                    self.play_sound("block.sculk_sensor.clicking_stop", [f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5], 1.0, pitch);
                }
            }
            _ => {}
        }
    }

    /// `SculkSensorBlock.getAnalogOutputSignal`: the last frequency while
    /// active.
    pub(super) fn sensor_output(&self, state: BlockStateId, pos: BlockPos) -> i32 {
        if self.sensor_phase(state) == "active" {
            self.vibration_listeners.get(&pos).map_or(0, |l| l.last_frequency)
        } else {
            0
        }
    }

    /// `SculkSensorBlock.getSignal`: its power, but a calibrated one gives
    /// none out of its facing side.
    pub(super) fn sensor_signal(&self, state: BlockStateId, direction: Direction) -> i32 {
        let calibrated = self.sensor_kind(state).is_some_and(|k| k.2);
        if calibrated && self.registries().blocks.property(state, "facing").and_then(Direction::from_name) == Some(direction) {
            return 0;
        }
        self.registries().blocks.property(state, "power").and_then(|p| p.parse().ok()).unwrap_or(0)
    }
}
