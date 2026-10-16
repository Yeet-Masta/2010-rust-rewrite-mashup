//! Beehives and bee nests on the server level (26.3 `BeehiveBlockEntity`):
//! the bees inside, their time in, and their way out.
//!
//! - A bee that goes in is kept as its saved data (less what does not
//!   outlast a stay: its motion, position, pollination counters, hive and
//!   UUID), with two minutes to stay if it brought nectar, half a minute
//!   if not; at most three fit.
//! - Each tick every bee's time in counts up; once past its stay it comes
//!   out of the hive's front, unless bees stay in (night or rain) or the
//!   front is blocked. One that brought nectar leaves it: the hive's honey
//!   rises by one (two, one time in a hundred) up to five.
//! - Coming out it remembers the hive, takes the hive's flower nine times
//!   in ten if it knows none, and has aged (or cooled off from love) by
//!   its time inside.
//! - Fire next to the hive drives every bee out at once.
//! - Now and then a full hive hums.

use super::Level;
use minecraftoss_core::nbt::Tag;
use minecraftoss_core::random::RandomSource;
use minecraftoss_core::{BlockPos, BlockStateId};
use std::collections::BTreeMap;

/// `BeehiveBlockEntity.MAX_OCCUPANTS`.
pub const MAX_OCCUPANTS: usize = 3;

/// `IGNORED_BEE_TAGS`: what a bee's data loses going in.
const IGNORED_BEE_TAGS: [&str; 25] = [
    "Air", "drop_chances", "equipment", "Brain", "CanPickUpLoot", "DeathTime", "fall_distance", "FallFlying", "Fire", "HurtTime", "LeftHanded", "Motion", "NoGravity", "OnGround",
    "PortalCooldown", "Pos", "Rotation", "sleeping_pos", "CannotEnterHiveTicks", "TicksSincePollination", "CropsGrownSincePollination", "hive_pos", "Passengers", "leash", "UUID",
];

/// Why a bee leaves (`BeeReleaseStatus`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Release {
    HoneyDelivered,
    BeeReleased,
    Emergency,
}

fn is_hive_id(id: &str) -> bool {
    matches!(id, "minecraft:beehive" | "minecraft:bee_nest")
}

impl Level<'_> {
    /// Every loaded hive and nest: where, how many bees are in, and
    /// whether fire burns beside it (`isFireNearby`).
    pub fn beehives(&self) -> Vec<((i32, i32, i32), usize, bool)> {
        let mut out = Vec::new();
        for chunk in self.chunks.values() {
            for (&key, tag) in &chunk.block_entities.entities {
                if !tag.get("id").and_then(Tag::as_str).is_some_and(is_hive_id) {
                    continue;
                }
                let count = tag.get("bees").and_then(Tag::as_list).map_or(0, <[Tag]>::len);
                let pos = BlockPos::new(key.0, key.1, key.2);
                out.push((key, count, self.fire_beside(pos)));
            }
        }
        out
    }

    /// Fire in the 3×3×3 about a hive.
    fn fire_beside(&self, pos: BlockPos) -> bool {
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if self.is_a(self.block(pos.offset(dx, dy, dz)), "FireBlock") {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// `BeehiveBlock.useItemOn` on a full hive: shears cut three combs
    /// loose, a bottle takes honey (the caller fills it); the honey resets.
    /// Unless smoke rises through it from a campfire below, the bees inside
    /// burst out. Whether smoke calmed it, when it was full.
    pub fn beehive_harvest(&mut self, pos: BlockPos, shears: bool) -> Option<bool> {
        let state = self.block(pos);
        if !self.is_a(state, "BeehiveBlock") || self.registries().blocks.property(state, "honey_level") != Some("5") {
            return None;
        }
        let at = [f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5];
        if shears {
            self.play_sound("block.beehive.shear", at, 1.0, 1.0);
            // `dropHoneycomb`: three combs.
            self.pop_resource(pos, super::container::Stack::new("minecraft:honeycomb", 3));
        } else {
            self.play_sound("item.bottle.fill", at, 1.0, 1.0);
        }
        let smoky = self.smoky_below(pos);
        if !smoky {
            self.beehive_release_all(pos);
        }
        let reset = self.with(self.block(pos), "honey_level", "0");
        self.set_block(pos, reset, 3, super::update::LIMIT);
        Some(smoky)
    }

    /// `CampfireBlock.isSmokeyPos`: a lit campfire within five blocks
    /// below, with nothing solid between.
    fn smoky_below(&self, pos: BlockPos) -> bool {
        for i in 1..=5 {
            let below = pos.offset(0, -i, 0);
            let state = self.block(below);
            if self.is_a(state, "CampfireBlock") && self.registries().blocks.property(state, "lit") == Some("true") {
                return true;
            }
            if self.block_collision_shape(state, below).is_some() && !self.is_a(state, "CampfireBlock") {
                return false;
            }
        }
        false
    }

    /// `emptyAllLivingFromHive` with `EMERGENCY`: every bee out now.
    pub fn beehive_release_all(&mut self, pos: BlockPos) {
        let state = self.block(pos);
        let Some(Tag::Compound(hive)) = self.block_entity(pos).cloned() else { return };
        let bees: Vec<Tag> = hive.get("bees").and_then(Tag::as_list).map(<[Tag]>::to_vec).unwrap_or_default();
        let flower = hive.get("flower_pos").cloned();
        let mut kept = Vec::new();
        for bee in bees {
            let Tag::Compound(map) = &bee else { continue };
            if !self.release_occupant(pos, state, map, Release::Emergency, flower.as_ref()) {
                kept.push(bee);
            }
        }
        if let Some(Tag::Compound(map)) = self.block_entity_mut(pos) {
            map.insert("bees".to_owned(), Tag::List(kept));
        }
    }

    /// `BeehiveBlockEntity.addOccupant`: a bee goes in, with its saved
    /// data. Whether there was room.
    pub fn beehive_add(&mut self, pos: BlockPos, mut bee: Tag) -> bool {
        let saved_flower = bee.get("flower_pos").cloned();
        let Some(Tag::Compound(hive)) = self.block_entity(pos).cloned() else { return false };
        let count = hive.get("bees").and_then(Tag::as_list).map_or(0, <[Tag]>::len);
        if count >= MAX_OCCUPANTS || !hive.get("id").and_then(Tag::as_str).is_some_and(is_hive_id) {
            return false;
        }
        let has_nectar = bee.get("HasNectar").and_then(Tag::as_i64).is_some_and(|n| n != 0);
        if let Tag::Compound(map) = &mut bee {
            for key in IGNORED_BEE_TAGS {
                map.remove(key);
            }
        }
        let occupant = Tag::Compound(BTreeMap::from([
            ("entity_data".to_owned(), bee),
            ("ticks_in_hive".to_owned(), Tag::Int(0)),
            ("min_ticks_in_hive".to_owned(), Tag::Int(if has_nectar { 2400 } else { 600 })),
        ]));
        let keeps_flower = hive.contains_key("flower_pos");
        let take_flower = saved_flower.is_some() && (!keeps_flower || self.random.next_bool());
        if let Some(Tag::Compound(map)) = self.block_entity_mut(pos) {
            match map.get_mut("bees") {
                Some(Tag::List(list)) => list.push(occupant),
                _ => {
                    map.insert("bees".to_owned(), Tag::List(vec![occupant]));
                }
            }
            if take_flower {
                if let Some(flower) = saved_flower {
                    map.insert("flower_pos".to_owned(), flower);
                }
            }
        }
        let at = [f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)];
        self.play_sound("block.beehive.enter", at, 1.0, 1.0);
        true
    }

    /// `BeehiveBlockEntity.serverTick`: the bees' time in, their leaving,
    /// fire's eviction and the hum.
    pub(super) fn beehive_tick(&mut self, pos: BlockPos) {
        let state = self.block(pos);
        let Some(Tag::Compound(hive)) = self.block_entity(pos).cloned() else { return };
        let mut bees: Vec<Tag> = hive.get("bees").and_then(Tag::as_list).map(<[Tag]>::to_vec).unwrap_or_default();
        let flower = hive.get("flower_pos").cloned();
        if bees.is_empty() {
            return;
        }
        // `setChanged` with fire beside: everyone out.
        let emergency = self.fire_beside(pos);
        let mut kept = Vec::new();
        let mut changed = false;
        for mut bee in bees.drain(..) {
            let Tag::Compound(map) = &mut bee else { continue };
            let ticks = map.get("ticks_in_hive").and_then(Tag::as_i64).unwrap_or(0) as i32;
            let min = map.get("min_ticks_in_hive").and_then(Tag::as_i64).unwrap_or(600) as i32;
            map.insert("ticks_in_hive".to_owned(), Tag::Int(ticks + 1));
            let due = ticks > min;
            if emergency || due {
                let nectar = map.get("entity_data").and_then(|d| d.get("HasNectar")).and_then(Tag::as_i64).is_some_and(|n| n != 0);
                let status = if emergency { Release::Emergency } else if nectar { Release::HoneyDelivered } else { Release::BeeReleased };
                if self.release_occupant(pos, state, map, status, flower.as_ref()) {
                    changed = true;
                    continue;
                }
            }
            kept.push(bee);
        }
        if let Some(Tag::Compound(map)) = self.block_entity_mut(pos) {
            map.insert("bees".to_owned(), Tag::List(kept.clone()));
        }
        let _ = changed;
        if !kept.is_empty() && self.random.next_f64() < 0.005 {
            let at = [f64::from(pos.x) + 0.5, f64::from(pos.y), f64::from(pos.z) + 0.5];
            self.play_sound("block.beehive.work", at, 1.0, 1.0);
        }
    }

    /// `releaseOccupant`: out of the front, its stay reckoned up. Whether it
    /// left.
    fn release_occupant(&mut self, pos: BlockPos, state: BlockStateId, occupant: &BTreeMap<String, Tag>, status: Release, flower: Option<&Tag>) -> bool {
        if status != Release::Emergency && self.bees_stay_in_hive() {
            return false;
        }
        let facing = self.registries().blocks.property(state, "facing").and_then(minecraftoss_core::pos::Direction::from_name).unwrap_or(minecraftoss_core::pos::Direction::North);
        let front = pos.relative(facing, 1);
        let blocked = self.block_collision_shape(self.block(front), front).is_some();
        if blocked && status != Release::Emergency {
            return false;
        }
        let Some(Tag::Compound(mut data)) = occupant.get("entity_data").cloned() else { return false };
        let ticks = occupant.get("ticks_in_hive").and_then(Tag::as_i64).unwrap_or(0) as i32;
        let baby = data.get("Age").and_then(Tag::as_i64).is_some_and(|a| a < 0);
        // `setBeeReleaseData`: aged, and cooled off, by its stay.
        let age = data.get("Age").and_then(Tag::as_i64).unwrap_or(0) as i32;
        if age < 0 {
            data.insert("Age".to_owned(), Tag::Int((age + ticks).min(0)));
        } else if age > 0 {
            data.insert("Age".to_owned(), Tag::Int((age - ticks).max(0)));
        }
        let love = data.get("InLove").and_then(Tag::as_i64).unwrap_or(0) as i32;
        data.insert("InLove".to_owned(), Tag::Int((love - ticks).max(0)));
        data.insert("hive_pos".to_owned(), Tag::IntArray(vec![pos.x, pos.y, pos.z]));
        if let Some(flower) = flower {
            if !data.contains_key("flower_pos") && self.random.next_f32() < 0.9 {
                data.insert("flower_pos".to_owned(), flower.clone());
            }
        }
        if status == Release::HoneyDelivered {
            // `dropOffNectar`, then the honey.
            data.insert("HasNectar".to_owned(), Tag::Byte(0));
            data.insert("CropsGrownSincePollination".to_owned(), Tag::Int(0));
            if let Some(level) = self.registries().blocks.property(state, "honey_level").and_then(|l| l.parse::<i32>().ok()) {
                if level < 5 {
                    let mut increase = if self.random.next_i32_bound(100) == 0 { 2 } else { 1 };
                    if level + increase > 5 {
                        increase -= 1;
                    }
                    let next = self.with(state, "honey_level", &(level + increase).to_string());
                    self.set_block_and_update(pos, next);
                }
            }
        }
        // Its box: a bee's 0.7 by 0.6 (a baby's half).
        let (width, height) = if baby { (0.35, 0.3) } else { (0.7, 0.6) };
        let delta = if blocked { 0.0 } else { 0.55 + width / 2.0 };
        let (sx, _, sz) = facing.offset();
        let (sx, sz) = (f64::from(sx), f64::from(sz));
        let at = [f64::from(pos.x) + 0.5 + delta * sx, f64::from(pos.y) + 0.5 - height / 2.0, f64::from(pos.z) + 0.5 + delta * sz];
        let yaw = data.get("Rotation").and_then(Tag::as_list).and_then(|r| r.first()).and_then(Tag::as_f64).unwrap_or(0.0) as f32;
        let mut tag = Tag::Compound(data);
        let uuid = super::super::natural_spawner::SpawnLevel::next_uuid(self);
        minecraftoss_core::entity_data::place(&mut tag, at, yaw, 0.0, uuid);
        self.spawned.push(tag);
        let sound_at = [f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5];
        self.play_sound("block.beehive.exit", sound_at, 1.0, 1.0);
        true
    }
}
